"""Evidence committed on top of a candidate.

The evidence owner commits frames, benchmark records and the dated validation
entry as one child `E` of candidate `C`, and acceptance fast-forwards to `E`
instead of rebuilding `C` on a base that carries them. These checks bound what
`E` may change so that it adds records only: nothing the build reads, nothing
executable and nothing the security review would need to see.
"""

from __future__ import annotations

import os
from pathlib import Path, PurePosixPath
import re
from typing import Callable

from .git import clone, git
from .process import LoopError
from .security_review import candidate_requires_security
from .task_spec import Task, path_allowed


# Directories whose regular files an evidence commit may add, change or (only
# under the first) delete, always within the task's scope, plus the dated log.
EVIDENCE_DIRECTORIES = ("docs/evidence/", "docs/benchmarks/")
DELETABLE = ("docs/evidence/",)
VALIDATION_LOG = "docs/validation.md"
# What an evidence commit may hold: frames and plain records wherever it may
# write, and prose, tables and logs only among the benchmarks, where the
# repository keeps them; the dated log is the one other Markdown file.
RECORD_SUFFIXES = frozenset({".png", ".jpg", ".jpeg", ".webp", ".json", ".txt"})
BENCHMARK_SUFFIXES = RECORD_SUFFIXES | {".md", ".csv", ".tsv", ".log", ".jsonl"}
# Files an agent reads as instructions wherever they sit.
INSTRUCTION_NAMES = frozenset({"agents.md", "claude.md", "claude.local.md", "skill.md"})
SHA = re.compile(r"[0-9a-f]{40}|[0-9a-f]{64}")
_RAW = re.compile(r":([0-7]{6}) ([0-7]{6}) [0-9a-f]+ [0-9a-f]+ ([A-Z])")


def evidence_ref(task_id: str, evidence: str) -> str:
    """Where an attempt clone keeps a fetched evidence commit, one ref per sha."""
    return f"refs/gitturtle/evidence/{task_id}/{evidence}"


def fetch_evidence(repo: Path, source: Path, task_id: str, evidence: str) -> None:
    """Copy `evidence` and its history from the owner's repository without moving HEAD."""
    if not SHA.fullmatch(evidence):
        raise LoopError("an evidence commit is named by its full hexadecimal sha")
    if not source.is_dir():
        raise LoopError(f"evidence repository is not a directory: {source}")
    git(repo, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head", "--no-recurse-submodules",
        str(source.resolve()), f"+{evidence}:{evidence_ref(task_id, evidence)}")


def evidence_file_refusal(path: str, protected: Callable[[str], bool]) -> str | None:
    """Why an evidence commit may not hold `path`, or None.

    `protected` is the controller's own test for policy and control paths.
    """
    parts = path.split("/")
    if any(part.startswith(".") for part in parts):
        return "a hidden path component"
    if protected(path) or parts[-1].casefold() in INSTRUCTION_NAMES:
        return "an instruction or control file"
    if path == VALIDATION_LOG:
        return None
    suffix = PurePosixPath(path).suffix
    if suffix not in (BENCHMARK_SUFFIXES if path.startswith("docs/benchmarks/") else RECORD_SUFFIXES):
        return f"a {suffix or 'suffix-less'} file there"
    return None


def evidence_paths(repo: Path, task: Task, candidate: str, evidence: str, *, protected: Callable[[str], bool],
                   checkout: Path | None = None) -> list[str]:
    """Check that `evidence` only adds records on top of `candidate`; return the paths it changes.

    `repo` holds `evidence`: the attempt clone, or the owner's repository for a
    read-only check before a request is queued. The candidate's build inputs are
    read from `checkout` (default `repo`), which the caller has checked is clean
    at the candidate. `protected` names the controller's policy and control paths.
    """
    try:
        named = git(repo, "rev-parse", "--verify", "--quiet", "--end-of-options", evidence + "^{commit}").strip()
    except LoopError:
        named = ""
    if named != evidence:
        raise LoopError(f"evidence commit {evidence[:12]} is not a commit in {repo}")
    if git(repo, "rev-list", "--parents", "-n", "1", evidence, "--").split()[1:] != [candidate]:
        raise LoopError(f"evidence commit {evidence[:12]} must have the candidate {candidate[:12]} as its only parent")
    entries = git(repo, "diff", "--raw", "--no-ext-diff", "--no-textconv", "--no-renames", "-z", candidate, evidence).split("\0")
    if not entries or entries.pop() != "" or len(entries) % 2:
        raise LoopError("unreadable evidence commit diff")
    paths = []
    for header, path in zip(entries[::2], entries[1::2]):
        match = _RAW.fullmatch(header)
        if not match:
            raise LoopError(f"unreadable evidence commit entry for {path}")
        old, new, _ = match.groups()
        if {old, new} - {"000000", "100644"}:
            raise LoopError(f"evidence commit must hold regular non-executable files only: {path}")
        if not (path == VALIDATION_LOG or path.startswith(EVIDENCE_DIRECTORIES)) or not path_allowed(path, task.scope):
            raise LoopError(f"evidence commit changes {path}, outside the evidence paths its task's scope grants")
        if refusal := evidence_file_refusal(path, protected):
            raise LoopError(f"evidence commit may not hold {refusal}: {path}")
        if new == "000000" and not path.startswith(DELETABLE):
            raise LoopError(f"evidence commit deletes {path}; it may delete evidence files only")
        paths.append(path)
    if not paths:
        raise LoopError(f"evidence commit {evidence[:12]} changes nothing")
    if candidate_requires_security(repo, candidate, evidence, paths):
        raise LoopError("evidence commit holds files the security review would need to see; commit only frames, records and prose")
    if found := build_references(checkout or repo):
        raise LoopError(f"the candidate's build reads evidence paths, so an evidence commit cannot ride on it: {found[0]}")
    return paths


def checkout_evidence(attempt: Path, destination: Path, task_id: str, candidate: str, evidence: str,
                      author: tuple[str, str], *, owner: Path) -> None:
    """A private clone of the attempt, checked out at its evidence commit."""
    clone(attempt, destination, candidate, author, owner=owner)
    git(destination, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head", str(attempt),
        f"+{evidence}:{evidence_ref(task_id, evidence)}", owner=owner)
    git(destination, "switch", "--quiet", "--detach", evidence, owner=owner)


def _crate_root(source: Path, root: Path) -> Path:
    for directory in source.parents:
        if (directory / "Cargo.toml").is_file() or directory == root:
            return directory
    return root


# Enough of Rust's lexical grammar to find string literals and the invocations
# around them: comments and character literals are skipped so a quote inside
# them never pairs with another, and a lone `'` is a lifetime.
_TOKEN = re.compile(r"""
    (?P<comment>//[^\n]*|/\*.*?\*/)
  | (?P<raw>b?r(?P<hashes>\#*)"(?P<rawbody>.*?)"(?P=hashes))
  | (?P<string>b?"(?P<body>(?:[^"\\]|\\.)*)")
  | (?P<char>b?'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])')
  | (?P<word>[A-Za-z_][A-Za-z0-9_]*!?)
  | (?P<punct>[()\[\]{},=])
  | (?P<other>\s+|.)
""", re.S | re.X)
_OPEN = {"(": ")", "[": "]", "{": "}"}
_INCLUDES = {"include_str!", "include_bytes!", "include!"}
_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0"}


def _tokens(text: str) -> list[tuple[str, int, str]]:
    """(kind, offset, value) for every literal, word and bracket; a literal's value is its text."""
    tokens = []
    for match in _TOKEN.finditer(text):
        kind = match.lastgroup
        if kind == "raw":
            tokens.append(("literal", match.start(), match["rawbody"]))
        elif kind == "string":
            tokens.append(("literal", match.start(), re.sub(r"\\(.)", lambda escape: _ESCAPES.get(escape[1], escape[1]), match["body"])))
        elif kind in {"word", "punct"}:
            tokens.append((kind, match.start(), match[kind]))
    return tokens


def _argument(tokens: list, start: int) -> tuple[list, int]:
    """The tokens inside the brackets that open at `start`, and the index after them."""
    close, depth, index = _OPEN[tokens[start][2]], 0, start
    while index < len(tokens):
        kind, _, value = tokens[index]
        if kind == "punct" and value in _OPEN:
            depth += 1
        elif kind == "punct" and value in _OPEN.values():
            depth -= 1
            if depth == 0:
                return tokens[start + 1:index], index + 1
        index += 1
    return tokens[start + 1:], index


def _split(tokens: list) -> list[list]:
    """Top-level comma-separated items; a trailing comma adds none."""
    items, current, depth = [], [], 0
    for token in tokens:
        if token[0] == "punct" and token[2] in _OPEN:
            depth += 1
        elif token[0] == "punct" and token[2] in _OPEN.values():
            depth -= 1
        if depth == 0 and token[:1] == ("punct",) and token[2] == ",":
            items.append(current)
            current = []
        else:
            current.append(token)
    return items + ([current] if current else [])


def _include_path(argument: list, source: Path, crate: Path) -> tuple[Path, str] | None:
    """Where an include macro's argument points, when it is literals alone or joined to CARGO_MANIFEST_DIR."""
    values = [token[2] for token in argument]
    if [token[0] for token in argument] == ["literal"]:
        return source.parent, values[0]
    if values[:2] != ["concat!", "("] or values[-1] != ")":
        return None
    items = _split(argument[2:-1])
    base = source.parent
    if items and [token[2] for token in items[0]] == ["env!", "(", "CARGO_MANIFEST_DIR", ")"]:
        items, base = items[1:], crate
    if not items or any([token[0] for token in item] != ["literal"] for item in items):
        return None
    joined = "".join(item[0][2] for item in items)
    # CARGO_MANIFEST_DIR is joined by concatenation, so its path starts with a separator.
    return base, joined.lstrip("/") if base == crate else joined


def build_references(root: Path) -> list[str]:
    """Build inputs under `crates/` and `vendor/` that read what an evidence commit may change.

    Every `.rs` file is read. An `include_str!`, `include_bytes!` or `include!`
    resolves when its argument is one plain or raw literal (relative to the
    file) or a `concat!` of literals, optionally after
    `env!("CARGO_MANIFEST_DIR")`; one that cannot be resolved is reported as
    soon as any literal in it mentions `docs`. rust-embed's `folder = ...`
    resolves against its crate, and a build script's every literal against both
    its crate and the repository root, because a script can join either. A
    path naming the evidence paths, or the whole `docs/` directory above them,
    is reported; other documentation (a research note a test compares constants
    with) is not. A procedural macro that reads a computed path is not seen.
    """
    protected = [(root / path.rstrip("/")).resolve() for path in (*EVIDENCE_DIRECTORIES, VALIDATION_LOG)]
    above = (root / "docs").resolve()
    found = []
    for top in ("crates", "vendor"):
        for source in sorted((root / top).rglob("*.rs")):
            if source.is_symlink() or not source.is_file():
                continue
            text = source.read_text(encoding="utf-8", errors="replace")
            if source.name != "build.rs" and "include" not in text and "folder" not in text:
                continue  # nothing here can name a build input
            crate = _crate_root(source, root)
            tokens = _tokens(text)
            paths: list[tuple[int, Path, str]] = []
            index = 0
            while index < len(tokens):
                kind, offset, value = tokens[index]
                if kind == "word" and value in _INCLUDES and index + 1 < len(tokens) and tokens[index + 1][2] in _OPEN:
                    argument, index = _argument(tokens, index + 1)
                    resolved = _include_path(argument, source, crate)
                    if resolved is not None:
                        paths.append((offset, *resolved))
                    elif any(token[0] == "literal" and "docs" in token[2].casefold() for token in argument):
                        found.append(f"{source.relative_to(root).as_posix()}:{text.count(chr(10), 0, offset) + 1} "
                                     "reads an unresolved path that mentions docs")
                    continue
                if kind == "word" and value == "folder" and [token[2] for token in tokens[index + 1:index + 2]] == ["="] \
                        and index + 2 < len(tokens) and tokens[index + 2][0] == "literal":
                    paths.append((offset, crate, tokens[index + 2][2]))
                if kind == "literal" and source.name == "build.rs":
                    paths.extend((offset, base, value) for base in (crate, root))
                index += 1
            for offset, base, literal in paths:
                # A directive such as `cargo:rerun-if-changed=PATH` names its path after the `=`.
                named = literal.split("=", 1)[1] if literal.startswith("cargo:") and "=" in literal else literal
                resolved = Path(os.path.normpath(base.resolve() / named))
                if resolved == above or any(resolved == path or resolved.is_relative_to(path) for path in protected):
                    found.append(f"{source.relative_to(root).as_posix()}:{text.count(chr(10), 0, offset) + 1} reads {literal!r}")
    return sorted(set(found))
