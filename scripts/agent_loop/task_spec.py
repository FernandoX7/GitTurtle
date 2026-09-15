"""Bounded, status-free task contracts for the optional development loop.

Queue editing is feature intake. Acceptance belongs to the controller's results,
and declaring a validation profile is never evidence that its checks ran.
"""

from __future__ import annotations

from dataclasses import dataclass
from fnmatch import fnmatchcase
import json
from pathlib import Path
import re
from typing import Any, Sequence
import unicodedata


MAX_SPEC_BYTES = 2 * 1024 * 1024
MAX_TASKS = 200
PROFILES = frozenset({"docs", "tooling", "rust", "native", "performance", "package", "vendor"})
MANUAL_EVIDENCE = frozenset({"native", "performance", "package", "vendor"})
_ID = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}\Z")
_COMMIT = re.compile(
    r"(?:feat|fix|docs|refactor|perf|test|build|ci|chore|revert)"
    r"(?:\([A-Za-z0-9][A-Za-z0-9._/-]*\))?!?: \S(?:.*\S)?\Z"
)
_TASK_FIELDS = frozenset(
    {"id", "title", "description", "depends_on", "scope", "acceptance", "profiles", "commit"}
)
_RESERVED = frozenset({".git", ".local"})


@dataclass(frozen=True)
class Task:
    id: str
    title: str
    description: str
    depends_on: tuple[str, ...]
    scope: tuple[str, ...]
    acceptance: tuple[dict[str, str], ...]
    profiles: tuple[str, ...]
    commit: str


def _object(value: Any, fields: set[str] | frozenset[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    if set(value) != fields:
        raise ValueError(f"{label} must contain exactly: {', '.join(sorted(fields))}")
    return value


def _text(value: Any, label: str, maximum: int) -> str:
    if not isinstance(value, str) or not value.strip() or len(value) > maximum:
        raise ValueError(f"{label} must be nonempty text of at most {maximum} characters")
    if value != value.strip() or any(unicodedata.category(char) in {"Cc", "Cf", "Cs"} for char in value):
        raise ValueError(f"{label} must have no controls or surrounding whitespace")
    return value


def _identifier(value: Any, label: str) -> str:
    result = _text(value, label, 64)
    if not _ID.fullmatch(result):
        raise ValueError(f"{label} must use lowercase letters, digits, hyphens or underscores")
    return result


def _array(value: Any, label: str, maximum: int, minimum: int = 0) -> list[Any]:
    if not isinstance(value, list) or not minimum <= len(value) <= maximum:
        raise ValueError(f"{label} must be an array with {minimum}..{maximum} entries")
    return value


def _unique(values: Sequence[str], label: str) -> None:
    if len(set(values)) != len(values):
        raise ValueError(f"{label} contains duplicate entries")


def _path_parts(value: Any, label: str, maximum: int) -> tuple[str, ...]:
    text = _text(value, label, maximum)
    if "\\" in text or ":" in text or text.startswith("/"):
        raise ValueError(f"{label} must be a repository-relative POSIX path")
    parts = tuple(text.split("/"))
    if any(part in {"", ".", ".."} or part.casefold() in _RESERVED for part in parts):
        raise ValueError(f"{label} contains an unsafe path component")
    return parts


def _scope(value: Any, label: str) -> str:
    parts = _path_parts(value, label, 256)
    for part in parts:
        if "**" in part and part != "**":
            raise ValueError(f"{label}: ** must occupy an entire path component")
        # Broad wildcards are useful, but explicitly targeting reserved metadata
        # with a spelling such as .g* is not. Concrete paths are checked again.
        if part not in {"*", "**"} and any(
            fnmatchcase(name, part.casefold()) for name in _RESERVED | {".", ".."}
        ):
            raise ValueError(f"{label} targets reserved repository metadata")
    return value


def _task(value: Any, index: int) -> Task:
    label = f"tasks[{index}]"
    raw = _object(value, _TASK_FIELDS, label)
    task_id = _identifier(raw["id"], f"{label}.id")
    dependencies = tuple(
        _identifier(item, f"{task_id}.depends_on")
        for item in _array(raw["depends_on"], f"{task_id}.depends_on", MAX_TASKS)
    )
    _unique(dependencies, f"{task_id}.depends_on")
    scopes = tuple(
        _scope(item, f"{task_id}.scope")
        for item in _array(raw["scope"], f"{task_id}.scope", 50, 1)
    )
    _unique(scopes, f"{task_id}.scope")
    criteria = []
    for item in _array(raw["acceptance"], f"{task_id}.acceptance", 50, 1):
        criterion = _object(item, {"id", "description"}, f"{task_id}.acceptance")
        criteria.append({
            "id": _identifier(criterion["id"], f"{task_id}.acceptance.id"),
            "description": _text(criterion["description"], f"{task_id}.acceptance.description", 2048),
        })
    _unique([item["id"] for item in criteria], f"{task_id}.acceptance")
    profiles = tuple(
        _text(item, f"{task_id}.profiles", 32)
        for item in _array(raw["profiles"], f"{task_id}.profiles", len(PROFILES), 1)
    )
    _unique(profiles, f"{task_id}.profiles")
    if not set(profiles) <= PROFILES:
        raise ValueError(f"{task_id}.profiles contains an unsupported profile")
    commit = _text(raw["commit"], f"{task_id}.commit", 72)
    if not _COMMIT.fullmatch(commit):
        raise ValueError(f"{task_id}.commit must be a Conventional Commit subject")
    return Task(
        id=task_id,
        title=_text(raw["title"], f"{task_id}.title", 160),
        description=_text(raw["description"], f"{task_id}.description", 4096),
        depends_on=dependencies,
        scope=scopes,
        acceptance=tuple(criteria),
        profiles=profiles,
        commit=commit,
    )


def _validate_graph(tasks: Sequence[Task]) -> None:
    by_id = {task.id: task for task in tasks}
    if len(by_id) != len(tasks):
        raise ValueError("tasks contains duplicate IDs")
    for task in tasks:
        if task.id in task.depends_on:
            raise ValueError(f"{task.id} depends on itself")
        unknown = set(task.depends_on) - by_id.keys()
        if unknown:
            raise ValueError(f"{task.id} has unknown dependencies: {', '.join(sorted(unknown))}")
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(task_id: str) -> None:
        if task_id in visiting:
            raise ValueError(f"dependency cycle includes {task_id}")
        if task_id in visited:
            return
        visiting.add(task_id)
        for dependency in by_id[task_id].depends_on:
            visit(dependency)
        visiting.remove(task_id)
        visited.add(task_id)

    for task in tasks:
        visit(task.id)


def parse_spec(data: Any) -> list[Task]:
    """Validate a contract and its dependency graph, without creating state."""
    raw = _object(data, {"version", "tasks"}, "spec")
    if type(raw["version"]) is not int or raw["version"] != 1:
        raise ValueError("spec.version must be the integer 1")
    tasks = [_task(item, index) for index, item in enumerate(_array(raw["tasks"], "tasks", MAX_TASKS))]
    _validate_graph(tasks)
    return tasks


def _json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_spec(path: str | Path) -> list[Task]:
    """Load bounded UTF-8 JSON; reject duplicate keys before schema validation."""
    with Path(path).open("rb") as stream:
        content = stream.read(MAX_SPEC_BYTES + 1)
    if len(content) > MAX_SPEC_BYTES:
        raise ValueError(f"task spec exceeds {MAX_SPEC_BYTES} bytes")
    try:
        return parse_spec(json.loads(content.decode("utf-8"), object_pairs_hook=_json_object))
    except (UnicodeError, RecursionError) as error:
        raise ValueError("task spec must be bounded UTF-8 JSON") from error


def select_ready(tasks: Sequence[Task], accepted: set[str], blocked: set[str]) -> list[Task]:
    """Return all eligible tasks in contract order, skipping unavailable work."""
    return [
        task for task in tasks
        if task.id not in accepted and task.id not in blocked and set(task.depends_on) <= accepted
    ]


def path_allowed(path: str, scopes: Sequence[str]) -> bool:
    """Match POSIX path globs; * stays in one segment and ** spans segments.

    This checks path names, not filesystem targets. The controller must separately
    constrain symlinks and capture both source and destination paths of renames.
    """
    try:
        parts = _path_parts(path, "changed path", 4096)
        if isinstance(scopes, (str, bytes)):
            return False
        patterns = [tuple(_scope(pattern, "scope").split("/")) for pattern in scopes]
    except (ValueError, TypeError):
        return False

    for pattern in patterns:
        positions = {0}
        for part in pattern:
            if not positions:
                break
            if part == "**":
                positions = set(range(min(positions), len(parts) + 1))
            else:
                positions = {
                    index + 1 for index in positions
                    if index < len(parts) and fnmatchcase(parts[index], part)
                }
        if len(parts) in positions:
            return True
    return False


def required_evidence(task: Task) -> set[str]:
    """Required manual evidence kinds; profiles alone are never proof."""
    return set(task.profiles) & MANUAL_EVIDENCE
