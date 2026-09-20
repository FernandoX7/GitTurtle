"""Git operations confined to independent runner-owned clones."""

from __future__ import annotations

from contextlib import contextmanager
from contextvars import ContextVar
import os
from pathlib import Path
import re
import subprocess
import tempfile
import uuid
from typing import Callable, Sequence

from .process import EnvironmentBlocked, LoopError, run_process


_limits: ContextVar[tuple[Callable[[], float], Callable[[], bool]]] = ContextVar(
    "git_write_limits", default=(lambda: 120.0, lambda: False),
)


@contextmanager
def write_limits(timeout: Callable[[], float], stop: Callable[[], bool]):
    token = _limits.set((timeout, stop))
    try:
        yield
    finally:
        _limits.reset(token)


def git(repo: Path, *args: str, owner: Path | None = None) -> str:
    environment = os.environ.copy()
    # Git environment inherited from hooks/CI must never redirect this repository.
    for key in list(environment):
        if key.startswith("GIT_") and key not in {"GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM"}:
            environment.pop(key)
    environment.update(GIT_TERMINAL_PROMPT="0", GIT_OPTIONAL_LOCKS="0", GIT_NO_LAZY_FETCH="1")
    argv = ["git", *([] if args[0] == "check-ignore" else ["--literal-pathspecs"]), "-c", "core.fsmonitor=false", "-C", str(repo), *args]
    if args[0] in {"clone", "switch", "add", "commit", "fetch", "merge", "config", "remote"}:
        # Writes can invoke hooks/filters. Keep their process ownership journal
        # alongside the run so recovery waits for cleanup before inspecting Git.
        owner = owner or next((parent for parent in [repo, *repo.parents] if
            (parent / "state.json").is_file() and (parent / "accepted/.git").is_dir()), None)
        if owner is not None:
            directory = owner / "git-processes" / uuid.uuid4().hex
            directory.mkdir(parents=True)
            return write_git(argv, args[0], repo, directory, environment)
        with tempfile.TemporaryDirectory(prefix="gitturtle-git-") as temporary:
            return write_git(argv, args[0], repo, Path(temporary), environment)
    try:
        result = subprocess.run(
            argv, stdin=subprocess.DEVNULL,
            capture_output=True, timeout=120, env=environment,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise LoopError(f"Git command failed: {args[0]}") from error
    if result.returncode:
        raise LoopError(f"git {args[0]} failed: {result.stderr.decode('utf-8', 'replace').strip()}")
    return result.stdout.decode("utf-8", "surrogateescape")


def write_git(argv: list[str], command: str, repo: Path, directory: Path, environment: dict[str, str]) -> str:
    output = directory / "stdout.log"
    errors = directory / "stderr.log"
    timeout, stop = _limits.get()
    if stop() or timeout() <= 0:
        raise EnvironmentBlocked(f"git {command} deferred by stop or time budget")
    result = run_process(argv, repo, output, min(120, timeout()), env=environment, stderr_path=errors, stop=stop)
    if result.stopped:
        raise EnvironmentBlocked(f"git {command} interrupted: {result.stopped}; inspect {directory}")
    if result.returncode:
        raise LoopError(f"git {command} failed: {errors.read_text(encoding='utf-8', errors='replace').strip()}")
    return output.read_bytes().decode("utf-8", "surrogateescape")


def head(repo: Path) -> str:
    return git(repo, "rev-parse", "HEAD").strip()


def within(path: str, roots: Sequence[str]) -> bool:
    return any(path == root or path.startswith(root + "/") for root in roots)


def status_entries(repo: Path) -> list[tuple[str, str]]:
    """Porcelain `(XY, path)` pairs; untracked files are listed one by one, never as a directory."""
    output = git(repo, "status", "--porcelain=v1", "--untracked-files=all", "--no-renames", "-z")
    return [(entry[:2], entry[3:]) for entry in output.split("\0") if entry]


def clean(repo: Path, *, ignore_untracked: Sequence[str] = ()) -> bool:
    """No change at all, apart from untracked files under `ignore_untracked` roots."""
    return all(code == "??" and within(path, ignore_untracked) for code, path in status_entries(repo))


def source_root(path: Path) -> Path:
    root = Path(git(path.resolve(), "rev-parse", "--show-toplevel").strip()).resolve()
    if not clean(root):
        raise LoopError("source checkout is dirty; commit intended work first (nothing was changed)")
    if git(root, "ls-files", "--stage").find("160000 ") >= 0:
        raise LoopError("submodules require an interactive workflow; the runner does not initialize them")
    return root


def identity(repo: Path) -> tuple[str, str]:
    ident = git(repo, "var", "GIT_AUTHOR_IDENT").strip()
    match = re.fullmatch(r"(.+) <([^<>\n]+)> [0-9]+ [+-][0-9]{4}", ident)
    if not match:
        raise LoopError("configure a Git author identity before running the queue")
    return match[1], match[2]


def clone(source: Path, destination: Path, revision: str, author: tuple[str, str], *, owner: Path | None = None) -> None:
    if destination.exists() or destination.is_symlink():
        raise LoopError(f"checkout already exists: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    git(source, "clone", "--quiet", "--no-hardlinks", "--no-checkout", "--", str(source), str(destination), owner=owner)
    git(destination, "remote", "remove", "origin", owner=owner)
    git(destination, "switch", "--quiet", "--detach", revision, owner=owner)
    git(destination, "config", "user.name", author[0], owner=owner)
    git(destination, "config", "user.email", author[1], owner=owner)


def untracked_paths(repo: Path) -> list[str]:
    return list(filter(None, git(repo, "ls-files", "--others", "--exclude-standard", "-z").split("\0")))


def changed_paths(repo: Path, *, ignore_untracked: Sequence[str] = ()) -> list[str]:
    tracked = git(repo, "diff", "--no-ext-diff", "--no-textconv", "HEAD", "--name-only", "--no-renames", "-z").split("\0")
    untracked = [path for path in untracked_paths(repo) if not within(path, ignore_untracked)]
    return sorted(set(filter(None, tracked + untracked)))


def committed_paths(repo: Path, base: str, candidate: str) -> list[str]:
    return list(filter(None, git(repo, "diff", "--no-ext-diff", "--no-textconv", "--name-only", "--no-renames", "-z", base, candidate).split("\0")))


def commit(repo: Path, paths: list[str], subject: str, body: str) -> str:
    # Only a fresh isolated checkout reaches here. Rebuild its index explicitly;
    # never stage ignored output or adopt an agent-created commit.
    git(repo, "add", "--", *paths)
    git(repo, "diff", "--cached", "--check")
    git(repo, "commit", "--quiet", "-m", subject, "-m", body)
    return head(repo)
