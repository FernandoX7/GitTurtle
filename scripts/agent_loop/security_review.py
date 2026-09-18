"""Conservative security routing and candidate-bound review results.

These checks validate a review's identity and completeness, not its truth. The
independent reviewer must trace actual trust boundaries and retain evidence.
"""

from __future__ import annotations

from pathlib import Path, PurePosixPath
import re

from .git import git

from .process import LoopError, MalformedResponse
from .task_spec import Task


STRING = {"type": "string"}
STRINGS = {"type": "array", "items": STRING}
SECURITY_REVIEW_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["task_id", "base", "candidate", "verdict", "reviewed_paths", "coverage", "findings", "gaps"],
    "properties": {
        "task_id": {**STRING, "description": "The contract's id, exactly."},
        "base": {**STRING, "description": "The base sha you were given, exactly."},
        "candidate": {**STRING, "description": "The candidate sha you were given, exactly."},
        "verdict": {
            "type": "string", "enum": ["pass", "fail", "blocked"],
            "description": "pass requires no findings, no gaps, and every reviewed path covered; "
                           "fail requires at least one finding; blocked requires at least one gap.",
        },
        "reviewed_paths": {
            **STRINGS,
            "description": "Exactly the candidate's changed paths, each once, and nothing else.",
        },
        "coverage": {"type": "array", "items": {
            "type": "object", "additionalProperties": False,
            "required": ["boundary", "paths", "evidence"],
            "properties": {
                "boundary": {**STRING, "description": "The trust boundary this entry covers."},
                "paths": {
                    **STRINGS,
                    "description": "The changed paths from reviewed_paths this boundary covers, each once. "
                                   "Only those: name any unchanged file you consulted in evidence instead. "
                                   "Together the entries must cover every reviewed path.",
                },
                "evidence": {
                    **STRING,
                    "description": "What you traced or ran for this boundary, naming any unchanged file "
                                   "you consulted. Never empty.",
                },
            },
        }},
        "findings": {
            "description": "Security defects only. Required for fail; empty to pass.",
            "type": "array", "items": {
            "type": "object", "additionalProperties": False,
            "required": ["severity", "location", "attacker_control", "source", "sink", "impact", "evidence", "remediation"],
            "properties": {
                "severity": {"type": "string", "enum": ["critical", "high", "medium", "low"]},
                **{key: STRING for key in ("location", "attacker_control", "source", "sink", "impact", "evidence", "remediation")},
            },
        }},
        "gaps": {
            **STRINGS,
            "description": "Concrete things you could not verify and why. Required for blocked; empty to pass.",
        },
    },
}


def security_required(paths: list[str]) -> bool:
    """Skip only recognized prose and static artwork; unknown inputs review.

    The caller supplies Git's --no-renames diff so both old and new names of a
    move are considered, including deleted files. A path is not a security
    boundary: the reviewer still examines the actual changed bytes and callers.
    """
    if not paths:
        return True
    for value in paths:
        if not isinstance(value, str) or not value or "\\" in value:
            return True
        path = PurePosixPath(value)
        if path.name == "AGENTS.md" or path.is_absolute() or any(part in {"", ".", ".."} for part in value.split("/")):
            return True
        prose = (
            value.startswith("docs/") and path.suffix in {".md", ".txt", ".rst"}
        ) or value in {
            "README.md", "CONTRIBUTING.md", "CHANGELOG.md", "DESIGN.md", "LICENSE", "LICENSE.txt",
        }
        artwork = value.startswith("assets/") and path.suffix in {
            ".png", ".jpg", ".jpeg", ".webp", ".ico", ".icns",
        }
        if not prose and not artwork:
            return True
    return False


def candidate_requires_security(repo: Path, base: str, candidate: str, paths: list[str]) -> bool:
    """The prose/artwork exemption additionally requires ordinary file modes."""
    if security_required(paths):
        return True
    entries = git(repo, "diff", "--raw", "--no-ext-diff", "--no-textconv", "--no-renames", "-z", base, candidate).split("\0")
    if not entries or entries.pop() != "" or len(entries) % 2:
        return True
    seen = set()
    for header, path in zip(entries[::2], entries[1::2]):
        match = re.fullmatch(r":([0-7]{6}) ([0-7]{6}) [0-9a-f]+ [0-9a-f]+ [A-Z]", header)
        if not match or path in seen or any(mode not in {"000000", "100644"} for mode in match.groups()):
            return True
        seen.add(path)
    return seen != set(paths)


def _strings(value: object, label: str, *, nonempty: bool = False) -> list[str]:
    if not isinstance(value, list) or (nonempty and not value) or not all(
        isinstance(item, str) and item.strip() for item in value
    ):
        raise LoopError(f"invalid security review {label}")
    return value


def validate_security_review(value: dict, task: Task, base: str, candidate: str, paths: list[str]) -> str:
    if not isinstance(value, dict) or set(value) != set(SECURITY_REVIEW_SCHEMA["required"]):
        raise MalformedResponse("security review has missing or unknown fields")
    if value["task_id"] != task.id or value["base"] != base or value["candidate"] != candidate:
        raise LoopError("security review is for a different task, base or candidate")
    verdict = value["verdict"]
    if not isinstance(verdict, str) or verdict not in {"pass", "fail", "blocked"}:
        raise MalformedResponse("invalid security review verdict")
    reviewed = _strings(value["reviewed_paths"], "reviewed paths")
    expected = set(paths)
    if len(reviewed) != len(set(reviewed)) or set(reviewed) != expected:
        raise LoopError("security review paths must match the candidate exactly once")
    if not isinstance(value["coverage"], list):
        raise MalformedResponse("invalid security review coverage")
    covered: set[str] = set()
    for item in value["coverage"]:
        if not isinstance(item, dict) or set(item) != {"boundary", "paths", "evidence"}:
            raise MalformedResponse("invalid security review coverage entry")
        if not all(isinstance(item[key], str) and item[key].strip() for key in ("boundary", "evidence")):
            raise MalformedResponse("security review coverage needs a boundary and evidence")
        item_paths = _strings(item["paths"], "coverage paths", nonempty=True)
        if not set(item_paths) <= expected or len(item_paths) != len(set(item_paths)):
            raise LoopError("security review coverage contains unknown or duplicate paths")
        covered.update(item_paths)
    findings = value["findings"]
    if not isinstance(findings, list):
        raise MalformedResponse("invalid security review findings")
    fields = set(SECURITY_REVIEW_SCHEMA["properties"]["findings"]["items"]["required"])
    for finding in findings:
        if not isinstance(finding, dict) or set(finding) != fields:
            raise MalformedResponse("invalid security review finding")
        if not all(isinstance(finding[key], str) and finding[key].strip() for key in fields):
            raise MalformedResponse("security findings require control, source, sink, impact and evidence")
        if finding["severity"] not in {"critical", "high", "medium", "low"}:
            raise MalformedResponse("invalid security finding severity")
    gaps = _strings(value["gaps"], "gaps")
    if verdict == "pass" and (findings or gaps or not value["coverage"] or covered != expected):
        raise LoopError("passing security review contains findings, gaps or incomplete coverage")
    if verdict == "fail" and not findings:
        raise LoopError("failing security review requires an evidenced finding")
    if verdict == "blocked" and not gaps:
        raise LoopError("blocked security review requires a concrete evidence gap")
    return verdict
