"""Mechanical rebase of a pending candidate onto an advanced accepted head.

Accepting a task advances `accepted_head`, which leaves every other pending
candidate on an older base. When its patch still applies, the controller
replays it, and the evidence commit on top of it, instead of spending an
implementer session on the same patch. The replay happens in a fresh private
clone beside the attempt, so the old candidate's clone never moves; the runner
gates the replayed candidate there and adopts it in one state save.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
import re

from .evidence import VALIDATION_LOG, fetch_evidence
from .git import clone, git, head
from .process import EnvironmentBlocked, LoopError


# Tree modes a replayed candidate may not hold on either side: as for a new
# candidate, a stored symlink or submodule needs an interactive review.
LINK_MODES = {"120000", "160000"}
_RAW = re.compile(r":([0-7]{6}) ([0-7]{6}) [0-9a-f]+ [0-9a-f]+ [A-Z]")
# A replay that conflicts in many paths names only the first few.
NAMED_CONFLICTS = 5


def prepare(source: Path, destination: Path, accepted: Path, onto: str, candidate: str, evidence: str | None,
            task_id: str, author: tuple[str, str], *, owner: Path) -> None:
    """A private clone of the attempt, checked out at `onto` and holding the candidate and its evidence commit."""
    clone(source, destination, candidate, author, owner=owner)
    git(destination, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head", str(accepted), onto, owner=owner)
    if evidence:
        fetch_evidence(destination, source, task_id, evidence)
    git(destination, "switch", "--quiet", "--detach", onto, owner=owner)


def _picking(repo: Path) -> bool:
    try:
        git(repo, "rev-parse", "--verify", "--quiet", "CHERRY_PICK_HEAD")
    except LoopError:
        return False
    return True


def _pick(repo: Path, commit: str, *, owner: Path) -> tuple[str | None, list[str], str]:
    """Cherry-pick `commit` onto HEAD: the new commit, or None with the conflicting paths and why."""
    try:
        git(repo, "cherry-pick", "--no-rerere-autoupdate", commit, owner=owner)
    except EnvironmentBlocked:
        raise
    except LoopError as error:
        conflicts = [path for path in git(repo, "diff", "--name-only", "--diff-filter=U", "-z").split("\0") if path]
        if _picking(repo):
            git(repo, "cherry-pick", "--abort", owner=owner)
        if conflicts:
            named = ", ".join(conflicts[:NAMED_CONFLICTS]) + (f" and {len(conflicts) - NAMED_CONFLICTS} more" if len(conflicts) > NAMED_CONFLICTS else "")
            return None, conflicts, f"it conflicts with the new base in {named}"
        return None, [], "it does not apply to the new base: " + " ".join(str(error).split())[:240]
    return head(repo), [], ""


def replay(repo: Path, commit: str, *, owner: Path) -> tuple[str | None, str]:
    """Cherry-pick `commit` onto HEAD; return the new commit, or None and why it did not apply.

    The pick keeps the commit's author and message and commits with the
    clone's configured identity, as `git.commit` does. A conflict, or a patch
    the new base already holds, is aborted, so the clone is left clean where
    it started; a stop or exhausted time budget propagates.
    """
    picked, _, why = _pick(repo, commit, owner=owner)
    return picked, why


def replay_evidence(repo: Path, original: str, evidence: str, *, owner: Path) -> tuple[str | None, str]:
    """Cherry-pick the evidence commit onto the replayed candidate at HEAD.

    Every evidence commit adds a dated entry at the top of the validation log,
    so one replayed past another's conflicts there and nowhere else. Only then
    is it picked again with Git's `union` merge driver for that file, set in
    the clone's own `info/attributes`, and the log is rebuilt with each of its
    entries where it added them, above the entries the new base added there
    (`stack_insertions`), since the union drops a blank line both sides added
    and glues the two entries. The result must add exactly the commit's lines
    to the log and change every other path exactly as the commit did; anything
    else refuses it. On success HEAD is the replayed commit; otherwise HEAD is
    the candidate again.
    """
    candidate = head(repo)
    picked, conflicts, why = _pick(repo, evidence, owner=owner)
    if picked or conflicts != [VALIDATION_LOG]:
        return picked, why
    attributes = repo / ".git/info/attributes"
    if attributes.exists() or attributes.is_symlink():
        return None, why
    # A clone made from an `init.templateDir` without `info/` has none yet.
    attributes.parent.mkdir(exist_ok=True)
    attributes.write_text(f"/{VALIDATION_LOG} merge=union\n", encoding="utf-8")
    try:
        picked, _, why = _pick(repo, evidence, owner=owner)
    finally:
        attributes.unlink()
    if picked is None:
        return None, why
    try:
        stacked = stack_insertions(*(_lines(repo, revision) for revision in (original, candidate, evidence)),
                                   _hunks(repo, original, evidence), _hunks(repo, original, candidate))
        if stacked is None:
            raise LoopError(f"its {VALIDATION_LOG} change is not one the replay can stack: it removes or edits lines, "
                            "or adds them where the new base changed the log")
        if "".join(stacked) != git(repo, "show", f"{picked}:{VALIDATION_LOG}"):
            (repo / VALIDATION_LOG).write_bytes("".join(stacked).encode("utf-8", "surrogateescape"))
            git(repo, "add", "--", VALIDATION_LOG, owner=owner)
            git(repo, "commit", "--quiet", "--amend", "--no-edit", owner=owner)
            picked = head(repo)
        _same_change(repo, original, evidence, candidate, picked)
    except EnvironmentBlocked:
        raise
    except LoopError as error:
        git(repo, "switch", "--quiet", "--detach", candidate, owner=owner)
        return None, f"it conflicts with the new base in {VALIDATION_LOG} and its union replay was refused: {error}"
    return picked, ""


_HUNK = re.compile(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def _lines(repo: Path, revision: str) -> list[str]:
    """The validation log's lines at `revision`, with their endings; none when it is absent."""
    if not git(repo, "ls-tree", "-z", revision, "--", VALIDATION_LOG):
        return []
    return git(repo, "show", f"{revision}:{VALIDATION_LOG}").splitlines(keepends=True)


def _hunks(repo: Path, old: str, new: str) -> list[tuple[int, int, int, int]]:
    """The validation log's -U0 hunks old..new as (old start, old count, new start, new count)."""
    diff = git(repo, "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--unified=0", old, new, "--", VALIDATION_LOG)
    return [
        (int(match[1]), 1 if match[2] is None else int(match[2]), int(match[3]), 1 if match[4] is None else int(match[4]))
        for match in (_HUNK.match(line) for line in diff.split("\n")) if match
    ]


def stack_insertions(base: list[str], ours: list[str], theirs: list[str],
                     inserted: list[tuple[int, int, int, int]], changed: list[tuple[int, int, int, int]]) -> list[str] | None:
    """`ours` with every block `theirs` inserted into `base` placed where it was inserted, newest first.

    `inserted` and `changed` are the -U0 hunks base..theirs and base..ours. A
    block of `theirs` goes at its point in `base`, above whatever `ours`
    inserted at that same point, so a newer dated entry comes first. Returns
    None when `theirs` removes or edits a line, when one of its points falls
    inside lines `ours` replaced, or when the hunks do not rebuild `ours`.
    """
    blocks: dict[int, list[str]] = {}
    for old_start, old_count, new_start, new_count in inserted:
        if old_count:
            return None
        # `-a,0` inserts after line a, which is before the 0-based index a.
        blocks[old_start] = theirs[new_start - 1:new_start - 1 + new_count]
    edits: dict[int, tuple[int, list[str]]] = {}
    for old_start, old_count, new_start, new_count in changed:
        start = old_start if old_count == 0 else old_start - 1
        end = start + old_count
        if any(start < point < end for point in blocks):
            return None
        edits[start] = (end, ours[new_start - 1:new_start - 1 + new_count])

    def rebuild(stacked: dict[int, list[str]]) -> list[str]:
        result: list[str] = []
        index = 0
        while index <= len(base):
            result += stacked.get(index, [])
            if index in edits:
                end, lines = edits[index]
                result += lines
                if end > index:
                    index = end
                    continue
            if index < len(base):
                result.append(base[index])
            index += 1
        return result

    if rebuild({}) != ours:
        return None
    return rebuild(blocks)


def _added(repo: Path, old: str, new: str) -> tuple[list[str], list[str]]:
    """The validation log's removed and added lines old..new, in order."""
    diff = git(repo, "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--unified=0", old, new, "--", VALIDATION_LOG).split("\n")
    body = diff[next((index for index, line in enumerate(diff) if line.startswith("@@")), len(diff)):]
    return [line[1:] for line in body if line.startswith("-")], [line[1:] for line in body if line.startswith("+")]


def _same_change(repo: Path, original: str, evidence: str, candidate: str, replayed: str) -> None:
    """Refuse a replayed evidence commit that does not change what the original changed, and only that."""
    def paths(old: str, new: str) -> set[str]:
        return {path for path in git(repo, "diff", "--name-only", "--no-renames", "-z", old, new).split("\0") if path}

    changed = paths(original, evidence)
    if paths(candidate, replayed) != changed:
        raise LoopError("it changes other paths than the evidence commit")
    others = sorted(changed - {VALIDATION_LOG})
    if others and git(repo, "diff", "--name-only", "--no-renames", "-z", evidence, replayed, "--", *others):
        raise LoopError("its other paths differ from the evidence commit's")
    removed, added = _added(repo, original, evidence)
    if removed or _added(repo, candidate, replayed) != ([], added):
        raise LoopError(f"its {VALIDATION_LOG} lines are not exactly the ones the evidence commit added")


def link_paths(repo: Path, base: str, candidate: str) -> list[str]:
    """Paths base..candidate that are a symlink or a submodule on either side."""
    entries = git(repo, "diff", "--raw", "--no-ext-diff", "--no-textconv", "--no-renames", "-z", base, candidate).split("\0")
    if not entries or entries.pop() != "" or len(entries) % 2:
        raise LoopError("unreadable rebased candidate diff")
    found = []
    for header, path in zip(entries[::2], entries[1::2]):
        match = _RAW.fullmatch(header)
        if not match:
            raise LoopError(f"unreadable rebased candidate entry for {path}")
        if {match[1], match[2]} & LINK_MODES:
            found.append(path)
    return found


def fingerprint(repo: Path, base: str, candidate: str) -> str:
    """The patch base..candidate without positions, blob ids or context.

    Two candidates with one fingerprint add and remove the same lines, in the
    same order, in the same files with the same modes and binary content,
    wherever their hunks sit, so a review of one patch is a review of the
    other. Both sides are computed by the same Git with the same options, so
    configured diff settings cannot make them differ.
    """
    diff = git(repo, "diff", "--no-ext-diff", "--no-textconv", "--no-renames", "--no-color", "--binary",
               "--full-index", "--unified=0", base, candidate)
    # `index` lines carry blob ids, which change when the base changed the file
    # elsewhere; a hunk header carries positions and the enclosing function.
    # No content line looks like either: text starts with +, - or a space, and
    # a binary line with a length letter followed by base85, which has no space.
    kept = ["@@" if line.startswith("@@") else line for line in diff.split("\n") if not line.startswith("index ")]
    return hashlib.sha256("\n".join(kept).encode("utf-8", "surrogateescape")).hexdigest()
