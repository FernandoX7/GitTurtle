"""Fixture copies for scenarios whose route writes, and the changes each launch declares.

A scenario whose route makes Git writes through the app (delete a branch,
push) declares them in `writes` (validated by `scenario.fixture_writes`).
Every launch then opens its own fresh copy of the recipe build, never the
build itself: the repository with its local bare remotes and linked
worktrees, copied to `<fixtures>/<name>_copy/`, with every remote URL and
worktree link moved from the build to the copy. A remote URL that is not a
local path inside the build, or a URL rewrite (`url.*.insteadOf`), is refused
before anything is copied, and again in the copy. The build, bare remotes
included, keeps its own state check before and after every launch and before
and after each copy, and stays reusable.

Every launch's copy has the same path, so a frame that shows the repository or
a remote path is the same for each role, variant, run and re-check; a second
run of the scenario meanwhile is refused. The copy's state is read before
and after its launch (HEAD, status, index, every ref, special ref such as
ORIG_HEAD or FETCH_HEAD, reflog and configuration file, the worktree list and
linked worktrees, and each bare remote's refs, special refs, reflogs and
configuration) and compared with the role's declaration; both states and the
verdict go into the launch's flow-log.json and run.json. Anything undeclared
must stay as it was: only the repository's and each bare remote's own
`config` file can be declared, key by key. A ref or key declared to change
must have changed: one already at its target before the launch cannot be told
apart from no write.

Reflogs: a declared ref's own reflog may change with it (appended to, or
removed with a deleted ref), and HEAD's reflog only when the branch HEAD names
is declared; every other reflog must stay byte-identical, and no reflog may
be rewritten.

Retention: a copy is removed when its launch ended with the app exited, or
never started, and the copy changed as declared or not at all. A copy that
changed otherwise, or whose app may still be running, is kept for inspection
with the reason in its marker file, and refused, never deleted, until the
operator removes it. The copy and the bare copies switch automatic maintenance
off, so no detached Git process (a local push's receive-pack does not get the
app's `-c gc.auto=0`) outlives a launch to race its after-state or the removal.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
import uuid
from dataclasses import dataclass
from pathlib import Path

from . import recipe, runenv

MARKER = ".gitturtle-fixture-copy.json"
SUFFIX = "_copy"  # a recipe name has no underscore (scenario.IDENTIFIER), so no build can sit at a copy's path
DELETED = "deleted"
QUIET = (("gc.auto", "0"), ("gc.autoDetach", "false"), ("maintenance.auto", "false"))
QUIET_BARE = (*QUIET, ("receive.autoGc", "false"))
HELPER_CONFIG = "[gc]\n\tauto = 0\n\tautoDetach = false\n[maintenance]\n\tauto = false\n"
# Git's pseudo-refs, which `for-each-ref` does not list (refs.c `is_root_ref`), HEAD aside.
SPECIAL_REF = re.compile(r"[A-Z_]+_HEAD|AUTO_MERGE|BISECT_EXPECTED_REV|NOTES_MERGE_PARTIAL|NOTES_MERGE_REF|"
                         r"MERGE_AUTOSTASH")
RETENTION = ("removed after a launch whose app exited or never started with the copy changed as declared or "
             "untouched; otherwise kept for inspection and refused until removed")
EXITED, NEVER_STARTED, MAY_RUN = "exited", "never started", "may still be running"


def utc() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def git(repo: Path, *args: str, codes=(0,)) -> str:
    """Git in `repo` as the recipe builder runs it: no inherited configuration, prompts, network or maintenance."""
    with tempfile.TemporaryDirectory(prefix="gitturtle-fixture-copy-") as home:
        Path(home, ".gitconfig").write_text(HELPER_CONFIG)
        result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True,
                                env=recipe.isolated_env(Path(home)), stdin=subprocess.DEVNULL)
    if result.returncode not in codes:
        raise SystemExit(f"refusing: git {' '.join(args[:4])} in {repo} failed: "
                         f"{result.stderr.decode(errors='replace').strip()[:400]}")
    return result.stdout.decode(errors="surrogateescape")


def inside(path: Path, root: Path) -> bool:
    """Lexically inside `root` as given or as resolved (Git records resolved paths; /tmp is /private/tmp on macOS)."""
    path = Path(os.path.normpath(path))
    return runenv.within(path, root) or runenv.within(path, root.resolve())


# ---------- remote URLs ----------
def config_entries(repo: Path, pattern: str) -> list[tuple[str, str]]:
    """(key, value) of `repo`'s own configuration whose key matches `pattern`."""
    out = git(repo, "config", "--local", "--null", "--get-regexp", pattern, codes=(0, 1))
    return [tuple(item.partition("\n")[::2]) for item in out.split("\0") if item]


def remote_problem(url: str, root: Path) -> str | None:
    """Why `url` is not a local path inside `root`, or None."""
    if not url.startswith("/") or "://" in url:
        return "not an absolute local path"
    if not inside(Path(url), root):
        return f"outside {root}"
    if not runenv.within(Path(url).resolve(), root.resolve()):
        return f"resolves to {Path(url).resolve()}, outside {root}"
    return None


def local_remotes(repo: Path, root: Path) -> list[tuple[str, str]]:
    """`repo`'s remote URLs, refused unless each is a local path inside `root` and nothing rewrites them."""
    rewrites = sorted({key for key, _ in config_entries(repo, r"^url\.")})
    if rewrites:
        raise SystemExit(f"refusing: {repo} rewrites remote URLs ({', '.join(rewrites)}); a fixture's remotes are "
                         "local bare repositories inside it")
    entries = config_entries(repo, r"^remote\..*\.(url|pushurl)$")
    for key, url in entries:
        problem = remote_problem(url, root)
        if problem is not None:
            raise SystemExit(f"refusing: {key} of {repo} is {url!r}, {problem}; a fixture's remotes are local bare "
                             f"repositories inside {root}, never a network URL")
    return entries


# ---------- the plan and its copies ----------
@dataclass(frozen=True)
class Plan:
    """Where a writing scenario's launches get their copies of a recipe build, and what each role declares."""
    source: Path  # the recipe build's root, never opened by a launch
    root: Path  # each launch's copy, at the same path every time
    repository: str  # the repository's path in both
    remotes: dict  # recipe remote name -> its bare repository's path in both
    commits: tuple  # the recipe's commits, for `@N`
    state: dict  # the build's state as built, bare remotes included
    declarations: dict  # role -> declared changes

    def record(self) -> dict:
        """The run record's account of the copies."""
        return dict(mode="each launch opens its own fresh copy of the recipe build", copy=str(self.root),
                    repository=str(self.root / self.repository), declared=self.declarations, retention=RETENTION)

    def build_state(self) -> dict:
        return recipe.state(self.source / self.repository, {name: self.source / bare
                                                            for name, bare in self.remotes.items()})


def occupied(root: Path) -> None:
    if root.exists() or root.is_symlink():
        reason = ""
        with contextlib.suppress(OSError, ValueError, AttributeError):
            marker = json.loads((root / MARKER).read_text())
            reason = f" ({marker.get('kept_because') or 'a launch in progress'}, {marker.get('role')} " \
                     f"{marker.get('run_dir', '')})"
        raise SystemExit(f"refusing: {root} exists{reason}: a fixture copy kept for inspection after a launch, another "
                         "run's copy, or something else; it is never deleted here. Inspect it, then remove it to run "
                         "again")


def plan(spec: dict, manifest: dict | None) -> Plan | None:
    """The copies a scenario with `writes` needs, refused before any launch; None for a scenario that only reads."""
    if spec.get("writes") is None:
        return None
    if manifest is None:
        raise SystemExit("refusing: the scenario writes, so every launch opens its own copy of its recipe build; "
                         "a --fixture is never copied or written to")
    source = Path(manifest["root"])
    found = Plan(source=source, root=source.with_name(source.name + SUFFIX), repository=spec["fixture"]["repository"],
                 remotes={op["name"]: op["bare"] for op in spec["fixture"]["operations"] if op["action"] == "remote"},
                 commits=tuple(manifest["commits"]), state=manifest["state"], declarations=spec["writes"])
    for repo in (source / found.repository, *(source / bare for bare in found.remotes.values())):
        local_remotes(repo, source)
    occupied(found.root)
    return found


@dataclass
class Snapshot:
    record: dict  # what flow-log.json and run.json keep
    reflogs: dict  # "" (the repository) or a remote name -> {reflog path: bytes}, for the append-only check


@dataclass
class Copy:
    root: Path
    repository: Path
    remotes: dict  # remote name -> its bare copy
    token: str  # written to the marker; only a copy carrying it is removed
    declared: dict
    commits: tuple
    before: Snapshot


def moved(path: Path, plan: Plan) -> Path:
    """`path` inside the build, at the same place in the copy."""
    path = Path(os.path.normpath(path))
    for base in (plan.source, plan.source.resolve()):
        if runenv.within(path, base):
            return plan.root / path.relative_to(base)
    raise SystemExit(f"refusing: {path} is outside the recipe build {plan.source}, so a copy cannot move it")


def copy_tree(plan: Plan) -> None:
    for entry in sorted(plan.source.iterdir()):
        if entry.name in (recipe.MANIFEST, f".{recipe.MANIFEST}.partial"):
            continue
        target = plan.root / entry.name
        if entry.name == MARKER:
            raise SystemExit(f"refusing: the recipe build holds {entry}, the name a copy's own marker takes")
        if entry.is_symlink():
            raise SystemExit(f"refusing: {entry} is a symbolic link; a fixture copy never follows one")
        if entry.is_dir():
            shutil.copytree(entry, target, symlinks=True)
        elif entry.is_file():
            shutil.copy2(entry, target)
        else:
            raise SystemExit(f"refusing: {entry} is neither a directory nor a file")


def relink_worktrees(plan: Plan) -> None:
    """Point each linked worktree of the copy and its administrative entry at each other, never at the build."""
    admin = plan.root / plan.repository / ".git" / "worktrees"
    if not admin.is_dir():
        return
    for entry in sorted(admin.iterdir()):
        link = entry / "gitdir"
        if entry.is_symlink() or not link.is_file() or link.is_symlink():
            raise SystemExit(f"refusing: the copy's worktree entry {entry} has no gitdir file")
        recorded = link.read_text().strip()
        if os.path.isabs(recorded):
            dotgit = moved(Path(recorded), plan)
            link.write_text(f"{dotgit}\n")
        else:  # worktree.useRelativePaths: both ends moved together
            dotgit = Path(os.path.normpath(entry / recorded))
        if dotgit.is_symlink() or not dotgit.is_file() or not inside(dotgit, plan.root):
            raise SystemExit(f"refusing: the copy's linked worktree {entry.name} has no .git file at {dotgit}")
        content = dotgit.read_text()
        if not content.startswith("gitdir: "):
            raise SystemExit(f"refusing: {dotgit} is not a linked worktree's .git file")
        target = content[len("gitdir: "):].strip()
        if os.path.isabs(target):
            dotgit.write_text(f"gitdir: {moved(Path(target), plan)}\n")


def move_remotes(plan: Plan, repository: Path) -> None:
    """Rewrite the copy's remote URLs, which still name the build's bare repositories, to the bare copies."""
    entries = local_remotes(repository, plan.source)
    for key in dict.fromkeys(key for key, _ in entries):
        urls = [str(moved(Path(url), plan)) for name, url in entries if name == key]
        git(repository, "config", "--local", "--unset-all", key)
        for url in urls:
            git(repository, "config", "--local", "--add", key, url)


def worktree_paths(repository: Path) -> list[Path]:
    listing = git(repository, "worktree", "list", "--porcelain")
    return [Path(line[len("worktree "):]) for line in listing.splitlines() if line.startswith("worktree ")]


def fresh_copy(plan: Plan, role: str, run_dir: Path | None = None) -> Copy:
    """A new copy of the build for one launch of `role`, with its state before the launch."""
    occupied(plan.root)
    if plan.build_state() != plan.state:  # a changed build is never copied
        raise SystemExit(f"refusing: the recipe build {plan.source} changed since it was built; remove it to rebuild")
    try:
        plan.root.mkdir()
    except FileExistsError:
        occupied(plan.root)
        raise
    token = uuid.uuid4().hex
    marker = dict(made_by="scripts/native_qa/writes.py", source=str(plan.source), role=role,
                  run_dir=str(run_dir or ""), pid=os.getpid(), created_utc=utc(), token=token)
    (plan.root / MARKER).write_text(json.dumps(marker, indent=1))
    try:
        copy_tree(plan)
        relink_worktrees(plan)
        repository = plan.root / plan.repository
        bares = {name: plan.root / bare for name, bare in plan.remotes.items()}
        move_remotes(plan, repository)
        for repo in (repository, *bares.values()):
            local_remotes(repo, plan.root)
        for key, value in QUIET:
            git(repository, "config", "--local", key, value)
        for bare in bares.values():
            for key, value in QUIET_BARE:
                git(bare, "config", "--local", key, value)
        paths = worktree_paths(repository)
        outside = [str(path) for path in paths if not inside(path, plan.root)]
        if outside:
            raise SystemExit(f"refusing: the copy's worktrees {', '.join(outside)} are outside {plan.root}")
        for path in paths:  # the copy's files have new inodes and ctimes: refresh the stat data once, before `before`
            git(path, "status", "--porcelain=v1", "-z", "--untracked-files=all")
        if plan.build_state() != plan.state:
            raise SystemExit(f"refusing: the recipe build {plan.source} changed while it was copied")
        before = snapshot(repository, bares)
    except BaseException:
        with contextlib.suppress(OSError):
            remove(plan.root, token)
        raise
    return Copy(plan.root, repository, bares, token, plan.declarations[role], plan.commits, before)


def remove(root: Path, token: str) -> None:
    """Delete the copy that carries `token` in its marker; anything else is left where it is."""
    try:
        marker = json.loads((root / MARKER).read_text())
    except (OSError, ValueError) as error:
        raise OSError(f"{root} has no readable {MARKER} ({error}); left in place") from None
    if root.is_symlink() or not root.is_dir() or marker.get("token") != token:
        raise OSError(f"{root} is not the copy this launch made; left in place")
    shutil.rmtree(root)


def keep(copy: Copy, reason: str) -> None:
    with contextlib.suppress(OSError, ValueError):
        path = copy.root / MARKER
        marker = json.loads(path.read_text())
        marker.update(kept_because=reason, kept_utc=utc())
        path.write_text(json.dumps(marker, indent=1))


def discard(copy: Copy) -> str | None:
    """Remove the copy of a launch whose session never started; a message when it could not be removed."""
    try:
        remove(copy.root, copy.token)
    except OSError as error:
        return f"could not remove the unused fixture copy {copy.root}: {error}"
    return None


# ---------- state ----------
def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ref_map(repo: Path) -> dict[str, str]:
    out = recipe.read(repo, "for-each-ref", "--format=%(refname) %(objectname)")
    if out.startswith(b"error: "):
        raise SystemExit(f"cannot list the refs of {repo}: {out.decode(errors='replace')[:400]}")
    return dict(line.split(" ", 1) for line in out.decode(errors="surrogateescape").splitlines())


def config_map(path: Path) -> dict[str, list]:
    """Every key of one configuration file, includes not followed, with its values in order (None: no value)."""
    if not path.is_file():
        return {}
    out = git(path.parent, "config", "--file", str(path), "--null", "--list")
    values: dict[str, list] = {}
    for item in filter(None, out.split("\0")):
        key, newline, value = item.partition("\n")
        values.setdefault(key, []).append(value if newline else None)
    return values


def git_files(git_dir: Path) -> tuple[dict, dict]:
    """The digests of a Git directory's special refs and of every `config.worktree`, its linked worktrees' too."""
    special, config_files = {}, {}
    for directory in (git_dir, *sorted((git_dir / "worktrees").glob("*"))):
        if not directory.is_dir() or directory.is_symlink():
            continue
        prefix = "" if directory == git_dir else f"{directory.relative_to(git_dir)}/"
        for path in sorted(directory.iterdir()):
            if path.is_file() and not path.is_symlink():
                if SPECIAL_REF.fullmatch(path.name):
                    special[prefix + path.name] = sha256(path.read_bytes())
                elif path.name == "config.worktree":
                    config_files[prefix + path.name] = sha256(path.read_bytes())
    return special, config_files


def reflog_files(git_dir: Path) -> dict[str, bytes]:
    logs = git_dir / "logs"
    if not logs.is_dir():
        return {}
    return {str(path.relative_to(logs)): path.read_bytes() for path in sorted(logs.rglob("*"))
            if path.is_file() and not path.is_symlink()}


def reflog_record(logs: dict[str, bytes]) -> dict:
    return {path: dict(entries=data.count(b"\n"), sha256=sha256(data)) for path, data in logs.items()}


def head(repo: Path) -> dict:
    symbolic = recipe.read(repo, "symbolic-ref", "--quiet", "HEAD").decode(errors="replace").strip()
    return dict(symbolic=None if symbolic.startswith("error:") else symbolic,
                oid=recipe.read(repo, "rev-parse", "--verify", "--quiet", "HEAD").decode(errors="replace").strip())


def snapshot(repository: Path, remotes: dict[str, Path]) -> Snapshot:
    """The copy's HEAD, status, index, refs, special refs, reflogs, configuration and worktrees, and each bare
    copy's refs, special refs, reflogs and configuration."""
    common = recipe.read(repository, "rev-parse", "--path-format=absolute", "--git-common-dir").decode().strip()
    if common.startswith("error:") or not common:
        raise SystemExit(f"cannot find the Git directory of {repository}: {common[:400]}")
    common_dir = Path(common)
    logs = {"": reflog_files(common_dir)}
    special, config_files = git_files(common_dir)
    # The worktree list without its HEAD lines, which the HEAD and linked-worktree checks cover.
    listing = [line for line in recipe.read(repository, "worktree", "list", "--porcelain").decode(
        errors="surrogateescape").splitlines() if line and not line.startswith("HEAD ")]
    record = dict(head=head(repository), refs=ref_map(repository), special_refs=special,
                  reflogs=reflog_record(logs[""]), config=config_map(common_dir / "config"),
                  config_files=config_files, worktrees=listing, state=recipe.state(repository), remotes={})
    for name, bare in sorted(remotes.items()):
        logs[name] = reflog_files(bare)
        bare_special, bare_config_files = git_files(bare)
        record["remotes"][name] = dict(path=str(bare), refs=ref_map(bare), special_refs=bare_special,
                                       reflogs=reflog_record(logs[name]), config=config_map(bare / "config"),
                                       config_files=bare_config_files)
    return Snapshot(record, logs)


# ---------- the verdict ----------
def short(oid: str | None) -> str:
    return "absent" if oid is None else oid[:12]


def values(found: list | None) -> str:
    if found is None:
        return "unset"
    return repr(found[0]) if len(found) == 1 else json.dumps(found)


def compare(declared: dict, before: Snapshot, after: Snapshot, commits) -> dict:
    """Every unexpected or missing change of the copy against its declaration, and every change seen."""
    problems: list[str] = []
    changes: list[str] = []
    local = after.record["refs"]

    def target(value: str) -> str | None:
        if value.startswith("@"):
            return commits[int(value[1:])]
        return local.get(value) if value.startswith("refs/") else value

    def refs(label: str, was_refs: dict, now_refs: dict, items: dict) -> None:
        for name in sorted({*was_refs, *now_refs, *items}):
            was, now, shown = was_refs.get(name), now_refs.get(name), f"{label}{name}"
            if was != now:
                changes.append(f"{shown}: {short(was)} -> {short(now)}")
            if name not in items:
                if was != now:
                    problems.append(f"{shown}: unexpected change {short(was)} -> {short(now)}")
                continue
            if items[name] == DELETED:
                if was is None:
                    problems.append(f"{shown}: declared deleted, but it did not exist before the launch")
                elif now is not None:
                    problems.append(f"{shown}: missing deletion, still at {short(now)}")
                continue
            want = target(items[name])
            if want is None:
                problems.append(f"{shown}: declared equal to {items[name]}, which does not exist after the launch")
            elif was == want:
                problems.append(f"{shown}: already at {short(want)} ({items[name]}) before the launch, so the "
                                "declared change cannot be told apart from no write")
            elif now != want:
                problems.append(f"{shown}: missing change to {short(want)} ({items[name]}), found {short(now)}")

    def config(label: str, was_config: dict, now_config: dict, items: dict) -> None:
        for key in sorted({*was_config, *now_config, *items}):
            was, now, shown = was_config.get(key), now_config.get(key), f"{label}config {key}"
            if was != now:
                changes.append(f"{shown}: {values(was)} -> {values(now)}")
            if key not in items:
                if was != now:
                    problems.append(f"{shown}: unexpected change {values(was)} -> {values(now)}")
            elif items[key] == DELETED:
                if was is None:
                    problems.append(f"{shown}: declared deleted, but it was not set before the launch")
                elif now is not None:
                    problems.append(f"{shown}: missing deletion, still {values(now)}")
            elif was == items[key]:
                problems.append(f"{shown}: already {values(was)} before the launch, so the declared change cannot be "
                                "told apart from no write")
            elif now != items[key]:
                problems.append(f"{shown}: missing change to {values(items[key])}, found {values(now)}")

    def unchanged(label: str, what: str, was_files: dict, now_files: dict) -> None:
        for name in sorted({*was_files, *now_files}):
            if was_files.get(name) != now_files.get(name):
                how = "created" if name not in was_files else "removed" if name not in now_files else "rewritten"
                problems.append(f"{label}{what} {name}: unexpected change ({how}; it cannot be declared)")

    def reflogs(label: str, was_logs: dict, now_logs: dict, items: dict, head_moves: bool) -> None:
        for path in sorted({*was_logs, *now_logs}):
            was, now = was_logs.get(path), now_logs.get(path)
            if was == now:
                continue
            shown = f"{label}reflog {path}"
            declared_ref = head_moves if path == "HEAD" else path in items
            if not declared_ref:
                problems.append(f"{shown}: unexpected change (only a declared ref's reflog may change)")
            elif now is None:
                if items.get(path) == DELETED:
                    changes.append(f"{shown}: removed with its ref")
                else:
                    problems.append(f"{shown}: removed, though its ref is declared to move, not to be deleted")
            elif was is not None and not now.startswith(was):
                problems.append(f"{shown}: rewritten; a write only appends to a reflog")
            else:
                appended = now.count(b"\n") - (was or b"").count(b"\n")
                changes.append(f"{shown}: {appended} entries appended")

    was, now = before.record, after.record
    own_refs, own_config = declared.get("refs", {}), declared.get("config", {})
    refs("", was["refs"], now["refs"], own_refs)
    unchanged("", "special ref", was["special_refs"], now["special_refs"])
    config("", was["config"], now["config"], own_config)
    unchanged("", "configuration file", was["config_files"], now["config_files"])
    branch = now["head"]["symbolic"]
    head_moves = branch is not None and branch in own_refs and own_refs[branch] != DELETED
    if was["head"]["symbolic"] != branch:
        problems.append(f"HEAD: unexpected change from {was['head']['symbolic'] or 'detached'} to "
                        f"{branch or 'detached'}")
    elif not head_moves and was["head"]["oid"] != now["head"]["oid"]:
        problems.append(f"HEAD: unexpected change {short(was['head']['oid'])} -> {short(now['head']['oid'])}")
    for key, what in (("status_sha256", "status"), ("index_sha256", "index")):
        if was["state"][key] != now["state"][key]:
            problems.append(f"{what}: unexpected change (declared writes leave the checkout as it was)")
    if was["worktrees"] != now["worktrees"]:
        problems.append("worktree list: unexpected change of a worktree's path, branch or lock")
    for path in sorted({*was["state"]["linked_worktrees"], *now["state"]["linked_worktrees"]}):
        if was["state"]["linked_worktrees"].get(path) != now["state"]["linked_worktrees"].get(path):
            problems.append(f"linked worktree {path}: unexpected change of its HEAD, status or index")
    reflogs("", before.reflogs[""], after.reflogs[""], own_refs, head_moves)
    for name in sorted({*was["remotes"], *now["remotes"]}):
        items = declared.get("remotes", {}).get(name, {})
        bare_was, bare_now, label = was["remotes"][name], now["remotes"][name], f"{name}:"
        refs(label, bare_was["refs"], bare_now["refs"], items.get("refs", {}))
        unchanged(label, "special ref", bare_was["special_refs"], bare_now["special_refs"])
        config(label, bare_was["config"], bare_now["config"], items.get("config", {}))
        unchanged(label, "configuration file", bare_was["config_files"], bare_now["config_files"])
        reflogs(label, before.reflogs[name], after.reflogs[name], items.get("refs", {}), False)
    return dict(result="fail" if problems else "pass", problems=problems, changes=changes)


def conclude(copy: Copy, app: str) -> dict:
    """The copy after its launch against the declaration; the copy is then removed or kept (see the module).

    `app` is EXITED, NEVER_STARTED (no process was spawned) or MAY_RUN (one
    was, and SIGTERM did not end it).
    """
    entry = dict(copy=str(copy.root), repository=str(copy.repository), app=app, declared=copy.declared,
                 before=copy.before.record)
    try:
        after = snapshot(copy.repository, copy.remotes)
    except (SystemExit, Exception) as error:
        problem = f"the copy's state could not be read: {error if isinstance(error, SystemExit) else repr(error)}"
        entry.update(after=None, verdict=dict(result="fail", problems=[problem], changes=[]))
        changed = True
    else:
        entry.update(after=after.record, verdict=compare(copy.declared, copy.before, after, copy.commits))
        changed = after.record != copy.before.record
    reason = None
    if app == MAY_RUN:
        reason = "the app may still be running in it"
    elif changed and entry["verdict"]["result"] != "pass":
        reason = "its changes differ from the declaration" if app == EXITED else \
            "it changed although the app never started"
    if reason is not None:
        keep(copy, reason)
        entry.update(kept=True, kept_because=reason)
        return entry
    try:
        remove(copy.root, copy.token)
    except OSError as error:
        entry.update(kept=True, removal_error=f"could not remove the fixture copy {copy.root}: {error}; remove it "
                                               "before the next launch")
    else:
        entry["kept"] = False
    return entry


def describe(spec: dict) -> list[str]:
    """`scenario check`'s lines on a writing scenario."""
    def shown(value) -> str:
        return DELETED if value == DELETED else f"-> {value if isinstance(value, str) else values(value)}"

    lines = [f"writes: every launch opens its own copy of the recipe build, <fixtures>/{spec['fixture']['name']}"
             f"{SUFFIX}/"]
    for role, declared in spec["writes"].items():
        sections = [("", declared), *((f"{name}:", items) for name, items in declared["remotes"].items())]
        parts = [f"{label}{name} {shown(value)}" for label, items in sections for name, value in items["refs"].items()]
        parts += [f"{label}config {key} {shown(value)}" for label, items in sections
                  for key, value in items["config"].items()]
        lines.append(f"  {role}: {', '.join(parts) or 'no change'}; everything else unchanged")
    return lines
