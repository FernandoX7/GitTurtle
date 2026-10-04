#!/usr/bin/env python3
"""Land one task the unattended controller accepted, as a squash-merged pull request.

    python3 scripts/operator/land.py --run RUN --task TASK
        [--handoff-commit PATCHFILE | --no-handoff] [--dry-run] [--no-merge]

1. Read the run's state through `agent-loop.py status`, require the task
   `accepted`, and take `record.base..<accepted commit>` from the run's `accepted`
   checkout. The accepted commit is the evidence commit when the record has one,
   else the candidate.
2. Add the worktree `.local/land/<task>` on `claude/land-<task>` from a freshly
   fetched `origin/main`.
3. Fetch the accepted commits and cherry-pick each with `-x`, in order.
4. Resolve a `docs/validation.md` conflict by keeping both dated entries, newest
   first, and check that the added lines equal the original commit's. Any other
   conflict aborts the cherry-pick and stops.
5. Check patch identity: each landed commit changes the same paths as its
   accepted original, with the same modes, the same object for a binary file and,
   context aside, the same added and removed lines (`docs/validation.md` included).
   Before step 2, every task the run accepted ahead of this one must already be
   on origin/main: its subject, carrying the content of its whole accepted range.
6. Commit the optional HANDOFF patch (only `docs/development/HANDOFF.md` and the
   run's queue file, the bookkeeping a squash merge may carry).
7. Build the PR body from the attestations and the task contract; the title is
   the task's commit subject.
8. Push, open the PR, wait for its checks and squash-merge with
   `--delete-branch --match-head-commit <pushed head>` once every check passed;
   then fast-forward the main checkout if it is clean and on `main`, remove the
   worktree, and remove each `.local/evidence/<task>/src-cand-*` clone that is
   clean with its HEAD inside the landed range (any other is kept and named),
   and the task's rebuildable outputs there: `gitturtle-*` executables, `pkg-*`
   bundles, archives and extracts, and `.target-*` directories.
   When GitHub refuses the merge because main moved under the PR (strict required
   checks), merge a freshly fetched origin/main into the branch locally, refuse
   unless the PR's change against it still has the accepted range's patch identity
   (plus the HANDOFF commit), push that merge and wait for its checks, then merge
   that head; at most three times. A conflict only in docs/validation.md keeps
   both sides' entries as in step 4; any other (HANDOFF included) stops with the
   commands that finish by hand.

`--dry-run` stops after step 5, prints the rest, and removes the worktree and
branch it made. `--no-merge` stops once the PR exists. Nothing is force-pushed
and no history is rewritten; anything unexpected stops with a message and leaves
the worktree for inspection.
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

VALIDATION = "docs/validation.md"
HANDOFF = "docs/development/HANDOFF.md"
BRANCH_PREFIX = "claude/land-"
SHA = re.compile(r"[0-9a-f]{40}(?:[0-9a-f]{24})?")
TASK_ID = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}")
MONTHS = ("January", "February", "March", "April", "May", "June", "July", "August",
          "September", "October", "November", "December")
DATED = re.compile(r"## (" + "|".join(MONTHS) + r") ([1-9][0-9]?)(?![0-9])")
MAX_SPEC_BYTES = 2 * 1024 * 1024
MAX_PATCH_BYTES = 1024 * 1024
MAX_EVIDENCE_BYTES = 1024 * 1024
CHECK_POLL_SECONDS = 30
CHECKS_APPEAR_SECONDS = 15 * 60
CHECKS_TIMEOUT_SECONDS = 3 * 60 * 60
# How often a PR that fell behind main is brought up to date before landing stops.
MAX_BRANCH_UPDATES = 3
# gh's words when strict required checks refuse a head that is behind its base.
BEHIND_REFUSAL = re.compile(r"not up to date with the base branch")
# Git variables that would point a command at another repository or index.
FOREIGN_GIT = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
               "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_PREFIX")


class LandError(Exception):
    """A condition the operator has to look at before landing can go on."""


class MergeConflict(LandError):
    """Merging origin/main into the landing branch conflicted; the merge was aborted."""


def say(message: str) -> None:
    print(f"land: {message}", flush=True)


# --- Git -----------------------------------------------------------------------------------------

def _environment() -> dict[str, str]:
    env = {key: value for key, value in os.environ.items() if key not in FOREIGN_GIT}
    # Never prompt: a cherry-pick keeps its message, a push fails rather than asking.
    env.update(GIT_TERMINAL_PROMPT="0", GIT_EDITOR="true")
    return env


def git_result(repo: Path, *args: str, data: bytes | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(repo), *args], env=_environment(), input=data,
                          capture_output=True, check=False)


def git_raw(repo: Path, *args: str, data: bytes | None = None) -> bytes:
    result = git_result(repo, *args, data=data)
    if result.returncode:
        detail = result.stderr.decode("utf-8", "replace").strip()
        raise LandError(f"git {' '.join(args)} failed in {repo}: {detail}")
    return result.stdout


def git(repo: Path, *args: str, data: bytes | None = None) -> str:
    return git_raw(repo, *args, data=data).decode("utf-8", "surrogateescape")


def head(repo: Path) -> str:
    return git(repo, "rev-parse", "--verify", "HEAD^{commit}").strip()


def is_ancestor(repo: Path, ancestor: str, descendant: str) -> bool:
    result = git_result(repo, "merge-base", "--is-ancestor", ancestor, descendant)
    if result.returncode not in (0, 1):
        raise LandError(f"cannot compare {ancestor[:12]} and {descendant[:12]} in {repo}: "
                        + result.stderr.decode("utf-8", "replace").strip())
    return result.returncode == 0


def subject(repo: Path, commit: str) -> str:
    return git(repo, "log", "-1", "--format=%s", commit).strip()


# --- 1. The accepted range ------------------------------------------------------------------------

def accepted_commit(record: dict) -> str:
    """The commit acceptance fast-forwarded `accepted` to: the evidence commit, else the candidate."""
    evidence = record.get("evidence_commit")
    if isinstance(evidence, dict):
        evidence = evidence.get("commit")
    commit = evidence if evidence is not None else record.get("candidate")
    if not isinstance(commit, str) or not SHA.fullmatch(commit):
        raise LandError("the task record names no full candidate or evidence commit")
    return commit


def select_commits(accepted_repo: Path, record: dict) -> list[str]:
    """`record.base..<accepted commit>`, oldest first, as a single-parent chain on the base."""
    if record.get("status") != "accepted":
        raise LandError(f"the task is {record.get('status')!r}, not 'accepted'")
    if record.get("landed"):
        raise LandError(f"the task landed on the source branch before the run as "
                        f"{str(record['landed'])[:12]}; there is nothing to land")
    base, candidate = record.get("base"), record.get("candidate")
    for name, value in (("base", base), ("candidate", candidate)):
        if not isinstance(value, str) or not SHA.fullmatch(value):
            raise LandError(f"the task record has no full {name} commit")
    tip = accepted_commit(record)
    accepted_head = head(accepted_repo)
    if not is_ancestor(accepted_repo, tip, accepted_head):
        raise LandError(f"{tip[:12]} is not on the accepted checkout's HEAD {accepted_head[:12]}")
    commits, parent = [], base
    for line in git(accepted_repo, "rev-list", "--reverse", "--topo-order", "--parents",
                    f"{base}..{tip}").splitlines():
        sha, *parents = line.split()
        if parents != [parent]:
            raise LandError(f"{sha[:12]} is not the only child of {parent[:12]}: "
                            f"{base[:12]}..{tip[:12]} is not a linear chain on the base")
        commits.append(sha)
        parent = sha
    if not commits or commits[-1] != tip:
        raise LandError(f"{base[:12]}..{tip[:12]} is empty")
    if candidate not in commits:
        raise LandError(f"the candidate {candidate[:12]} is not in {base[:12]}..{tip[:12]}")
    if tip != candidate and commits[-2:] != [candidate, tip]:
        raise LandError(f"the evidence commit {tip[:12]} is not a child of the candidate {candidate[:12]}")
    return commits


def earlier_ranges(accepted_repo: Path, state: dict, task_id: str) -> list[tuple[str, list[str]]]:
    """The accepted tasks whose whole ranges make up the run's chain from `source_head` to the
    task's base, in chain order; refuses a commit there that no accepted task accounts for."""
    source, base = state.get("source_head"), state["tasks"][task_id]["base"]
    if not isinstance(source, str) or not SHA.fullmatch(source):
        raise LandError("the run state names no full source_head")
    chain = git(accepted_repo, "rev-list", "--reverse", "--topo-order", f"{source}..{base}").split()
    owners: dict[str, str] = {}
    ranges: dict[str, list[str]] = {}
    for other_id, other in state["tasks"].items():
        if other_id == task_id or not isinstance(other, dict) or other.get("status") != "accepted" \
                or other.get("landed"):
            continue
        try:
            ranges[other_id] = select_commits(accepted_repo, other)
        except LandError:
            continue  # it cannot be in the chain; a chain commit nobody owns is refused below
        owners.update(dict.fromkeys(ranges[other_id], other_id))
    result: list[tuple[str, list[str]]] = []
    for commit in chain:
        owner = owners.get(commit)
        if owner is None:
            raise LandError(f"{commit[:12]}, between the run's source {source[:12]} and this task's base "
                            f"{base[:12]}, belongs to no accepted task of the run; land it by hand first")
        if not result or result[-1][0] != owner:
            result.append((owner, []))
        result[-1][1].append(commit)
    for owner, commits in result:
        if commits != ranges[owner]:
            raise LandError(f"the accepted range of {owner} does not lie whole below {base[:12]}")
    return result


# --- 4. The docs/validation.md two-entry merge ----------------------------------------------------

def split_sections(text: str) -> tuple[str, list[str]]:
    """The text above the first `## ` heading, then each `## ` section with the lines that follow it."""
    preamble: list[str] = []
    sections: list[list[str]] = []
    for line in text.splitlines(keepends=True):
        if line.startswith("## "):
            sections.append([line])
        elif sections:
            sections[-1].append(line)
        else:
            preamble.append(line)
    return "".join(preamble), ["".join(section) for section in sections]


def heading(section: str) -> str:
    return section.split("\n", 1)[0].rstrip("\r")


def entry_date(section: str) -> tuple[int, int]:
    match = DATED.match(section)
    if not match:
        raise LandError(f"validation section {heading(section)!r} has no '## <Month> <day>' date")
    return MONTHS.index(match[1]) + 1, int(match[2])


def newer(first: str, second: str) -> bool:
    """Whether entry `first` is dated after entry `second` (headings carry no year)."""
    (first_month, first_day), (second_month, second_day) = entry_date(first), entry_date(second)
    if abs(first_month - second_month) > 6:  # across a new year: January follows December
        return first_month < second_month
    return (first_month, first_day) > (second_month, second_day)


def inserted_entries(parent: str, picked: str) -> list[tuple[str | None, str | None, list[str]]]:
    """Each run of sections `picked` adds to `parent`, with the headings before and after it.

    Refuses any other change, since only whole added entries can be merged mechanically."""
    parent_preamble, parent_sections = split_sections(parent)
    picked_preamble, picked_sections = split_sections(picked)
    if parent_preamble != picked_preamble:
        raise LandError(f"the picked commit changes the text above the first section of {VALIDATION}")
    runs: list[tuple[str | None, str | None, list[str]]] = []
    run: list[str] = []
    previous: str | None = None
    index = 0
    for section in picked_sections:
        if index < len(parent_sections) and section == parent_sections[index]:
            if run:
                runs.append((previous, heading(section), run))
                run = []
            previous, index = heading(section), index + 1
        else:
            run.append(section)
    if index != len(parent_sections):
        raise LandError(f"the picked commit edits or removes an existing section of {VALIDATION}")
    if run:
        runs.append((previous, None, run))
    if not runs:
        raise LandError(f"the picked commit adds no section to {VALIDATION}")
    return runs


def _anchor(sections: list[str], name: str) -> int:
    matches = [index for index, section in enumerate(sections) if heading(section) == name]
    if len(matches) != 1:
        state = "missing from" if not matches else "ambiguous on"
        raise LandError(f"the section {name!r} is {state} the landing branch's {VALIDATION}")
    return matches[0]


def merge_validation(ours: str, parent: str, picked: str) -> str:
    """`ours` with the entries `picked` adds to `parent`, placed among the entries `ours` gained
    at the same place: newest first, and the picked entry first on the same date, since it lands last."""
    preamble, result = split_sections(ours)
    for previous, following, new in inserted_entries(parent, picked):
        for section in new:
            entry_date(section)
            if section in result:
                raise LandError(f"{heading(section)!r} is already on the landing branch")
        start = 0 if previous is None else _anchor(result, previous) + 1
        end = len(result) if following is None else _anchor(result, following)
        if end < start:
            raise LandError(f"the sections around {heading(new[0])!r} moved on the landing branch")
        merged: list[str] = []
        rest = result[start:end]
        for section in new:
            while rest and newer(rest[0], section):
                merged.append(rest.pop(0))
            merged.append(section)
        result[start:end] = merged + rest
    return preamble + "".join(result)


# --- 5. Patch identity ------------------------------------------------------------------------------

def changed_lines(diff: str) -> tuple[list[str], list[str]]:
    """The added and removed lines of a unified diff, read only inside its hunks."""
    added: list[str] = []
    removed: list[str] = []
    in_hunk = False
    for line in diff.split("\n"):
        if line.startswith("diff --git "):
            in_hunk = False
        elif line.startswith("@@"):
            in_hunk = True
        elif in_hunk and line.startswith("+"):
            added.append(line[1:])
        elif in_hunk and line.startswith("-"):
            removed.append(line[1:])
    return added, removed


def same_lines(first: list[str], second: list[str]) -> bool:
    """The same lines in the same order, allowing blank lines to sit where each diff placed them."""
    return (Counter(first) == Counter(second)
            and [line for line in first if line.strip()] == [line for line in second if line.strip()])


def changed_paths(repo: Path, old: str, new: str) -> set[str]:
    output = git(repo, "diff-tree", "-r", "-z", "--name-only", "--no-commit-id", old, new)
    return {path for path in output.split("\0") if path}


def file_change(repo: Path, old: str, new: str, path: str) -> tuple[tuple[str, ...], str | None, list[str], list[str]]:
    """How `path` changes from `old` to `new`, without context lines, so a nearby edit on another
    base does not count: its mode lines, a binary file's new object id, and the added and removed lines."""
    diff = git(repo, "diff-tree", "-p", "-U0", "--binary", "--full-index", old, new, "--", f":(literal){path}")
    modes: list[str] = []
    blob, binary = None, False
    for line in diff.split("\n"):
        if line.startswith("@@"):
            break
        if line.startswith(("new file mode ", "deleted file mode ", "old mode ", "new mode ")):
            modes.append(line)
        elif line.startswith("index "):
            blob = line.split()[1].split("..")[-1]
        elif line.startswith(("GIT binary patch", "Binary files ")):
            binary = True
    added, removed = changed_lines(diff)
    return tuple(modes), blob if binary else None, added, removed


def same_change(first: tuple, second: tuple) -> bool:
    return (first[:2] == second[:2] and same_lines(first[2], second[2]) and same_lines(first[3], second[3]))


def change_problems(first_repo: Path, first: tuple[str, str], second_repo: Path, second: tuple[str, str],
                    extra: frozenset[str] = frozenset()) -> list[str]:
    """How the change `second` (old, new) differs from `first`: paths, modes, binary objects and the
    lines each text file adds and removes. `second` may also change the paths in `extra`."""
    expected = changed_paths(first_repo, *first)
    actual = changed_paths(second_repo, *second)
    problems = []
    if expected - actual:
        problems.append("misses " + ", ".join(sorted(expected - actual)))
    if actual - expected - extra:
        problems.append("also changes " + ", ".join(sorted(actual - expected - extra)))
    for path in sorted(expected & actual):
        if not same_change(file_change(first_repo, *first, path), file_change(second_repo, *second, path)):
            problems.append(f"changes {path} differently")
    return problems


def identity_problems(accepted_repo: Path, originals: list[str], landing_repo: Path,
                      landed: list[str]) -> list[str]:
    if len(originals) != len(landed):
        return [f"{len(landed)} landed commits for {len(originals)} accepted ones"]
    problems = []
    for original, copy in zip(originals, landed):
        problems += [f"{copy[:12]} (from {original[:12]}) {problem}" for problem in
                     change_problems(accepted_repo, (f"{original}^", original), landing_repo, (f"{copy}^", copy))]
    return problems


def updated_problems(accepted_repo: Path, originals: list[str], landing_repo: Path, base: str, updated: str,
                     handoff: str | None, bookkeeping: set[str]) -> list[str]:
    """How the PR's change from origin/main `base` to the `updated` head differs from the accepted range
    followed by the HANDOFF commit, which alone may change the `bookkeeping` paths, and only as it did.

    One diff cannot tell the two apart in a path both edit, so then it fails closed, although the
    per-commit check of the first push passed: such a task is landed by hand."""
    allowed = frozenset(bookkeeping) if handoff else frozenset()
    if handoff:
        both = (changed_paths(accepted_repo, f"{originals[0]}^", originals[-1])
                & changed_paths(landing_repo, f"{handoff}^", handoff))
        if both:
            return [f"the accepted range and the HANDOFF commit {handoff[:12]} both edit {', '.join(sorted(both))}, "
                    "which this check cannot separate; land it by hand"]
    problems = change_problems(accepted_repo, (f"{originals[0]}^", originals[-1]), landing_repo, (base, updated),
                               allowed)
    if handoff:
        accepted = changed_paths(accepted_repo, f"{originals[0]}^", originals[-1])
        paths = (changed_paths(landing_repo, f"{handoff}^", handoff)
                 | changed_paths(landing_repo, base, updated)) & allowed
        problems += [f"changes {path} differently from the HANDOFF commit {handoff[:12]}"
                     for path in sorted(paths - accepted)
                     if not same_change(file_change(landing_repo, f"{handoff}^", handoff, path),
                                        file_change(landing_repo, base, updated, path))]
    return problems


# --- 3. Cherry-picks --------------------------------------------------------------------------------

def cherry_pick(worktree: Path, commit: str) -> bool:
    """Cherry-pick `commit` with -x; True when docs/validation.md needed the two-entry merge."""
    before = head(worktree)
    result = git_result(worktree, "-c", "rerere.enabled=false", "cherry-pick", "-x", commit)
    merged = False
    if result.returncode:
        unmerged = sorted(path for path in git(worktree, "diff", "--name-only", "-z",
                                                "--diff-filter=U").split("\0") if path)
        try:
            if unmerged != [VALIDATION]:
                detail = result.stderr.decode("utf-8", "replace").strip().splitlines()
                raise LandError(f"the cherry-pick of {commit[:12]} stopped"
                                + (f" with conflicts in {', '.join(unmerged)}" if unmerged
                                   else f": {detail[-1] if detail else 'no reason given'}")
                                + "; resolve it by hand in a fresh worktree, or re-cut the task")
            text = merge_validation(git(worktree, "show", f"HEAD:{VALIDATION}"),
                                    git(worktree, "show", f"{commit}^:{VALIDATION}"),
                                    git(worktree, "show", f"{commit}:{VALIDATION}"))
        except LandError:
            git_result(worktree, "cherry-pick", "--abort")
            raise
        (worktree / VALIDATION).write_bytes(text.encode("utf-8", "surrogateescape"))
        git(worktree, "add", "--", VALIDATION)
        git(worktree, "-c", "core.editor=true", "cherry-pick", "--continue")
        merged = True
    after = head(worktree)
    if git(worktree, "rev-list", "--parents", "-n", "1", after).split()[1:] != [before]:
        raise LandError(f"the cherry-pick of {commit[:12]} did not leave one new commit on {before[:12]}")
    if f"(cherry picked from commit {commit})" not in git(worktree, "log", "-1", "--format=%B", after):
        raise LandError(f"{after[:12]} does not record `cherry picked from commit {commit[:12]}`")
    if merged:
        if not same_change(file_change(worktree, f"{commit}^", commit, VALIDATION),
                           file_change(worktree, f"{after}^", after, VALIDATION)):
            raise LandError(f"the merged {VALIDATION} in {after[:12]} adds other lines than {commit[:12]}")
    return merged


# --- 6. HANDOFF commit ----------------------------------------------------------------------------------

def patch_paths(text: str) -> set[str]:
    paths: set[str] = set()
    for line in text.split("\n"):
        if line.startswith("diff --git "):
            match = re.fullmatch(r"diff --git a/(\S+) b/(\S+)", line.rstrip("\r"))
            if not match:
                raise LandError("the HANDOFF patch names a quoted or spaced path; make it with `git diff`")
            paths.update(match.groups())
    if not paths:
        raise LandError("the HANDOFF patch changes no file")
    return paths


def is_mailbox(text: str) -> bool:
    header = text.split("\n\n", 1)[0]
    return text.startswith("From ") and "\nSubject: " in header


def read_handoff(path: Path, allowed: set[str]) -> tuple[Path, str]:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_PATCH_BYTES:
        raise LandError(f"{path} is not a regular patch file of at most {MAX_PATCH_BYTES} bytes")
    text = path.read_text(encoding="utf-8", errors="surrogateescape")
    outside = sorted(patch_paths(text) - allowed)
    if outside:
        raise LandError(f"the HANDOFF patch may change only {', '.join(sorted(allowed))}, "
                        f"so that the squash still lands the task; it also changes {', '.join(outside)}")
    return path.resolve(), text


def apply_handoff(worktree: Path, patch: Path, text: str, task_id: str, allowed: set[str]) -> str:
    before = head(worktree)
    if is_mailbox(text):
        git(worktree, "am", "--quiet", str(patch))
    else:
        git(worktree, "apply", "--index", str(patch))
        git(worktree, "commit", "--quiet", "-m", f"docs(dev): record the landing of {task_id}")
    after = head(worktree)
    if git(worktree, "rev-list", "--parents", "-n", "1", after).split()[1:] != [before]:
        raise LandError("the HANDOFF patch did not make one commit")
    outside = sorted(changed_paths(worktree, f"{after}^", after) - allowed)
    if outside:
        raise LandError(f"the HANDOFF commit {after[:12]} changes {', '.join(outside)}")
    return after


# --- 7. PR body ---------------------------------------------------------------------------------------

def load_task(run: Path, task_id: str) -> dict:
    spec = run / "tasks.json"
    if spec.is_symlink() or not spec.is_file() or spec.stat().st_size > MAX_SPEC_BYTES:
        raise LandError(f"{spec} is not the run's task snapshot")
    data = json.loads(spec.read_text(encoding="utf-8"))
    tasks = data.get("tasks") if isinstance(data, dict) else data
    for task in tasks if isinstance(tasks, list) else []:
        if isinstance(task, dict) and task.get("id") == task_id:
            if not all(isinstance(task.get(key), str) and task[key] for key in ("title", "commit")):
                raise LandError(f"the contract of {task_id} has no title or commit subject")
            return task
    raise LandError(f"{task_id} is not in the run's task snapshot")


def read_attestations(run: Path, record: dict) -> list[tuple[str, dict, dict | None]]:
    """(kind, the controller's attestation record, the owner's JSON evidence when it is a JSON object)."""
    result = []
    attestations = record.get("attestations") if isinstance(record.get("attestations"), dict) else {}
    for kind, item in sorted(attestations.items()):
        if not isinstance(item, dict):
            continue
        document = None
        artifact = item.get("artifact")
        if isinstance(artifact, str):
            path = run / artifact
            try:
                if (not path.is_symlink() and path.is_file() and path.stat().st_size <= MAX_EVIDENCE_BYTES
                        and path.resolve().is_relative_to(run.resolve())):
                    parsed = json.loads(path.read_text(encoding="utf-8"))
                    document = parsed if isinstance(parsed, dict) else None
            except (OSError, ValueError):
                document = None
        result.append((kind, item, document))
    return result


def _short(value: object) -> str:
    text = str(value)
    return text[:12] if SHA.fullmatch(text) else text


def pr_body(task: dict, record: dict, *, run_id: str, queue: str, commits: list[tuple[str, str]],
            attestations: list[tuple[str, dict, dict | None]],
            replacements: tuple[tuple[str, str], ...] = ()) -> str:
    """The PR description: what landed, how it was accepted, and the owner's evidence.

    Only named attestation fields are used, and `replacements` turns local paths into
    repository-relative or home-relative ones, so the squash commit carries no host layout."""

    def clean(value: object) -> str:
        text = " ".join(str(value).split())
        for old, new in replacements:
            text = text.replace(old, new)
        return text

    candidate = record["candidate"]
    tip = accepted_commit(record)
    required = [kind for kind in record.get("required_evidence", []) if isinstance(kind, str)]
    accepted_at = str(record.get("accepted_at", ""))[:16].replace("T", " ")
    checks = ["The controller's gates", "the independent verifier"]
    if record.get("security_required"):
        checks.append("the security review")
    if required:
        checks.append("the required evidence (" + ", ".join(f"`{kind}`" for kind in required) + ")")
    lines = [
        "## Summary", "", clean(task["title"]).rstrip(".") + ".", "",
        f"Task `{task['id']}` of `{queue}`, accepted in run `{run_id}`"
        + (f" at {accepted_at} UTC" if accepted_at else "")
        + f": candidate `{candidate[:12]}`" + (f", evidence commit `{tip[:12]}`" if tip != candidate else "")
        + ". " + ", ".join(checks[:-1]) + " and " + checks[-1] + " passed.",
        "",
        "Commits, cherry-picked with `-x` from the run's `accepted` checkout; each changes the same files "
        "with the same added and removed lines as its original, and binary files to the same objects:",
        "",
    ]
    lines += [f"- `{clean(text)}` (`{sha[:12]}`)" for sha, text in commits]
    if attestations:
        lines += ["", "## Evidence"]
    for kind, item, document in attestations:
        evidence = document or {}
        lines += ["", f"### {kind.capitalize()}", ""]
        if item.get("summary"):
            lines += [clean(item["summary"]), ""]
        identity = [f"candidate `{_short(evidence.get('candidate') or item.get('candidate') or candidate)}`"]
        for key, label in (("base", "base"), ("evidence_commit", "evidence commit")):
            if evidence.get(key):
                identity.append(f"{label} `{_short(evidence[key])}`")
        lines.append("- Identity: " + ", ".join(identity))
        executable = evidence.get("attested_executable")
        if isinstance(executable, dict) and executable.get("sha256"):
            build = " ".join(filter(None, [
                str(executable.get("profile") or ""),
                f"build of `{_short(executable['source_revision'])}`" if executable.get("source_revision") else "",
            ]))
            details = [part for part in (build, f"source tree {executable['source_tree']}"
                                         if executable.get("source_tree") else "") if part]
            lines.append(f"- Executable: sha256 `{clean(executable['sha256'])}`"
                         + (f" ({clean(', '.join(details))})" if details else ""))
        for key, label in (("host", "Host"), ("input", "Input"), ("committed_frames", "Committed frames")):
            if evidence.get(key):
                lines.append(f"- {label}: {clean(evidence[key])}")
        analyses = evidence.get("analyses")
        if isinstance(analyses, dict) and isinstance(analyses.get("total"), int):
            lines.append(f"- Analyses: {clean(analyses.get('as_expected'))} of {analyses['total']} as expected")
        sessions = [session for session in evidence.get("sessions", []) if isinstance(session, dict)]
        if sessions:
            lines.append("- Sessions:")
            lines += [f"  - {clean(session.get('utc', ''))}: {clean(session.get('what', ''))}"
                      for session in sessions]
        if evidence.get("limitations"):
            lines.append(f"- Limitations: {clean(evidence['limitations'])}")
    return "\n".join(lines) + "\n"


# --- 8. GitHub ----------------------------------------------------------------------------------------

def gh(cwd: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(["gh", *args], cwd=cwd, env=_environment(), capture_output=True, text=True,
                            check=False)
    if check and result.returncode:
        raise LandError(f"gh {' '.join(args[:3])} failed: {(result.stderr or result.stdout).strip()}")
    return result


def pr_checks(cwd: Path, number: str) -> list[dict]:
    result = gh(cwd, "pr", "checks", number, "--json", "name,bucket,link", check=False)
    try:
        checks = json.loads(result.stdout) if result.stdout.strip() else []
    except ValueError:
        raise LandError(f"gh pr checks printed no JSON: {result.stderr.strip()}") from None
    if not checks and result.returncode and "no checks" not in result.stderr:
        raise LandError(f"gh pr checks failed: {result.stderr.strip()}")
    return [check for check in checks if isinstance(check, dict)]


def runs_finished(cwd: Path, sha: str) -> bool:
    """Every workflow run on the head finished well; checks of jobs not yet queued are not listed."""
    result = gh(cwd, "run", "list", "--commit", sha, "--json", "status,conclusion,workflowName")
    runs = json.loads(result.stdout or "[]")
    bad = [run for run in runs if run.get("status") == "completed"
           and run.get("conclusion") not in ("success", "skipped", "neutral")]
    if bad:
        raise LandError("workflow run failed: " + ", ".join(f"{run.get('workflowName')} ({run.get('conclusion')})"
                                                             for run in bad))
    return bool(runs) and all(run.get("status") == "completed" for run in runs)


def wait_for_checks(cwd: Path, number: str, sha: str) -> int:
    started = time.monotonic()
    last = ""
    while True:
        checks = pr_checks(cwd, number)
        failed = [check for check in checks if check.get("bucket") in ("fail", "cancel")]
        if failed:
            raise LandError("checks failed: " + "; ".join(f"{check.get('name')} {check.get('link', '')}".strip()
                                                         for check in failed))
        pending = [check for check in checks if check.get("bucket") not in ("pass", "skipping")]
        if checks and not pending and runs_finished(cwd, sha):
            return len(checks)
        buckets = Counter(str(check.get("bucket")) for check in checks)
        summary = ", ".join(f"{count} {bucket}" for bucket, count in sorted(buckets.items())) or "none reported"
        if summary != last:
            say(f"checks: {summary}")
            last = summary
        elapsed = time.monotonic() - started
        if not checks and elapsed > CHECKS_APPEAR_SECONDS:
            raise LandError(f"no checks reported on PR #{number} after {CHECKS_APPEAR_SECONDS // 60} minutes")
        if elapsed > CHECKS_TIMEOUT_SECONDS:
            raise LandError(f"checks still pending on PR #{number} after {CHECKS_TIMEOUT_SECONDS // 60} minutes")
        time.sleep(CHECK_POLL_SECONDS)


def pr_view(cwd: Path, number: str, fields: str) -> dict:
    """`gh pr view --json fields`, or {} when gh prints no JSON object."""
    result = gh(cwd, "pr", "view", number, "--json", fields, check=False)
    try:
        view = json.loads(result.stdout) if result.stdout.strip() else {}
    except ValueError:
        return {}
    return view if isinstance(view, dict) else {}


def merge_main(worktree: Path, branch: str, main_sha: str) -> tuple[str, bool]:
    """Merge origin/main `main_sha` into `branch`, checked out in `worktree`: (the merge commit, which stays
    local; whether docs/validation.md needed the two-entry merge). A conflict only in docs/validation.md
    keeps both sides' entries as the cherry-pick does, with the branch's placed as if it landed last on
    main; any other conflict aborts the merge, leaving the branch where it was."""
    before = head(worktree)
    message = f"Merge origin/main {main_sha[:12]} into {branch}"
    result = git_result(worktree, "-c", "rerere.enabled=false", "merge", "--no-ff", "--no-edit",
                        "-m", message, main_sha)
    resolved = False
    if result.returncode:
        unmerged = sorted(path for path in git(worktree, "diff", "--name-only", "-z",
                                                "--diff-filter=U").split("\0") if path)
        stopped = f"merging origin/main {main_sha[:12]} into {branch} stopped"
        try:
            if unmerged != [VALIDATION]:
                detail = result.stderr.decode("utf-8", "replace").strip().splitlines()
                raise MergeConflict(stopped + (f" with conflicts in {', '.join(unmerged)}" if unmerged
                                               else f": {detail[-1] if detail else 'no reason given'}"))
            fork = git(worktree, "merge-base", before, main_sha).strip()
            try:
                text = merge_validation(git(worktree, "show", f"{main_sha}:{VALIDATION}"),
                                        git(worktree, "show", f"{fork}:{VALIDATION}"),
                                        git(worktree, "show", f"{before}:{VALIDATION}"))
            except LandError as error:
                raise MergeConflict(f"{stopped} with conflicts in {VALIDATION} that the two-entry merge "
                                    f"cannot resolve: {error}") from None
        except LandError:
            git_result(worktree, "merge", "--abort")
            raise
        (worktree / VALIDATION).write_bytes(text.encode("utf-8", "surrogateescape"))
        git(worktree, "add", "--", VALIDATION)
        git(worktree, "commit", "--quiet", "-m", message)
        resolved = True
    after = head(worktree)
    if git(worktree, "rev-list", "--parents", "-n", "1", after).split()[1:] != [before, main_sha]:
        raise LandError(f"merging origin/main {main_sha[:12]} did not leave one merge commit on {before[:12]}")
    return after, resolved


def update_branch(root: Path, worktree: Path, branch: str, pushed: str, base: str) -> tuple[str, str]:
    """Check `branch` out again at the `pushed` head and merge a freshly fetched origin/main into it,
    which must descend from `base`: (that origin/main, the local merge commit)."""
    git(root, "worktree", "add", "--quiet", str(worktree), branch)
    if head(worktree) != pushed:
        raise LandError(f"{branch} is at {head(worktree)[:12]}, not the pushed {pushed[:12]}")
    git(root, "fetch", "--quiet", "origin", "main")
    main_sha = git(root, "rev-parse", "--verify", "refs/remotes/origin/main^{commit}").strip()
    if not is_ancestor(root, base, main_sha):
        raise LandError(f"origin/main {main_sha[:12]} does not descend from {base[:12]}, the PR's base")
    if is_ancestor(root, main_sha, pushed):
        raise LandError(f"GitHub reports the PR behind main, yet origin/main {main_sha[:12]} is in {pushed[:12]}")
    say(f"worktree {worktree} back on {branch} at {pushed[:12]}; merging origin/main {main_sha[:12]}")
    merged, resolved = merge_main(worktree, branch, main_sha)
    say(f"merged origin/main {main_sha[:12]} into {branch} as {merged[:12]}, not pushed yet")
    if resolved:
        say(f"  {VALIDATION}: kept both sides' entries, newest first, the landing one first on a shared date")
    return main_sha, merged


def push_target(worktree: Path, touches_workflows: bool) -> str:
    """origin, or its SSH URL when workflows change: the gh HTTPS token lacks the `workflow` scope."""
    if not touches_workflows:
        return "origin"
    url = git(worktree, "remote", "get-url", "origin").strip()
    match = re.fullmatch(r"https://github\.com/([^/\s]+)/([^/\s]+?)(?:\.git)?/?", url)
    return f"git@github.com:{match[1]}/{match[2]}.git" if match else "origin"


def push_by_hand(repo: Path, branch: str, since: str, touches_workflows: bool) -> str:
    """The push of a merge of origin/main made by hand on top of `since`, by push_target's rule: over SSH
    when the task's commits or what the merge brings in change workflows, which only the merge shows."""
    ssh = push_target(repo, True)
    if touches_workflows or ssh == "origin":
        return f"`git push {ssh} HEAD:{branch}`"
    return (f"`git push origin HEAD:{branch}`, or `git push {ssh} HEAD:{branch}` if "
            f"`git diff --name-only {since[:12]} HEAD -- .github/workflows` lists a file")


# --- Orchestration --------------------------------------------------------------------------------------

def main_checkout(start: Path) -> Path:
    common = Path(git(start, "rev-parse", "--path-format=absolute", "--git-common-dir").strip())
    if common.name != ".git":
        raise LandError(f"{common} is not a checkout's .git directory")
    return common.parent


def resolve_run(root: Path, value: str) -> Path:
    path = Path(value).expanduser()
    if not path.is_dir() and not path.is_absolute() and (root / ".local/agent-loop" / value).is_dir():
        path = root / ".local/agent-loop" / value
    if not path.is_dir() or not (path / "state.json").is_file():
        raise LandError(f"{value} is not a run directory")
    return path.resolve()


def run_status(script_root: Path, run: Path) -> dict:
    result = subprocess.run([sys.executable, str(script_root / "scripts/agent-loop.py"), "status", "--run", str(run)],
                            capture_output=True, text=True, check=False)
    if result.returncode:
        raise LandError(f"agent-loop.py status failed: {result.stderr.strip()}")
    try:
        state = json.loads(result.stdout)
    except ValueError:
        raise LandError("agent-loop.py status printed no JSON state") from None
    if not isinstance(state, dict) or not isinstance(state.get("tasks"), dict):
        raise LandError("the run state has no task records")
    return state


def landed_as(repo: Path, tip: str, title: str) -> str | None:
    output = git(repo, "log", "--format=%H%x00%s", "--fixed-strings", f"--grep={title}", tip)
    for line in output.splitlines():
        sha, _, text = line.partition("\0")
        if text == title or re.fullmatch(re.escape(title) + r" \(#[0-9]+\)", text):
            return sha
    return None


def earlier_landed(root: Path, main_sha: str, run: Path, state: dict, task_id: str) -> list[str]:
    """Where each task accepted ahead of this one sits on origin/main.

    A squash carries no `cherry picked from` trailer, so a task counts as landed when a commit has
    its subject (with an optional ` (#N)`) and, context aside, the content of its whole accepted
    range, plus at most the HANDOFF and queue bookkeeping."""
    accepted_repo = run / "accepted"
    extra = frozenset({HANDOFF, str(state.get("spec_path", HANDOFF))})
    found = []
    for other_id, commits in earlier_ranges(accepted_repo, state, task_id):
        title = load_task(run, other_id)["commit"]
        squash = landed_as(root, main_sha, title)
        if not squash:
            raise LandError(f"{other_id}, accepted ahead of {task_id} in this run, is not on origin/main "
                            f"as {title!r}; land it first")
        problems = change_problems(accepted_repo, (f"{commits[0]}^", commits[-1]), root, (f"{squash}^", squash), extra)
        if problems:
            raise LandError(f"{squash[:12]} on origin/main has the subject of {other_id} but not its accepted "
                            f"range {commits[0][:12]}..{commits[-1][:12]}: " + "; ".join(problems))
        found.append(f"{other_id} as {squash[:12]}")
    return found


def remove_worktree(root: Path, worktree: Path, branch: str) -> None:
    if worktree.exists():
        git(root, "worktree", "remove", str(worktree))
    git(root, "worktree", "prune")
    if git_result(root, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}").returncode == 0:
        git(root, "branch", "--quiet", "-D", branch)


def clone_keep_reason(clone: Path, accepted_repo: Path, tip: str) -> str | None:
    """Why a candidate source clone must stay, or None when it holds nothing the landing did not carry:
    it is clean and its HEAD is the landed tip or one of its ancestors."""
    if clone.is_symlink() or not clone.is_dir() or not (clone / ".git").exists():
        return "it is not a Git checkout"
    status = git_result(clone, "status", "--porcelain")
    if status.returncode or status.stdout.strip():
        return "it has local changes or untracked files"
    resolved = git_result(clone, "rev-parse", "--verify", "HEAD^{commit}")
    if resolved.returncode:
        return "it has no HEAD commit"
    clone_head = resolved.stdout.decode().strip()
    if git_result(accepted_repo, "merge-base", "--is-ancestor", clone_head, tip).returncode != 0:
        return f"its HEAD {clone_head[:12]} is not the landed {tip[:12]} or an ancestor"
    return None


def remove_source_clones(root: Path, task_id: str, accepted_repo: Path, tip: str) -> tuple[list[Path], list[str]]:
    evidence = root / ".local/evidence" / task_id
    removed, kept = [], []
    for clone in sorted(evidence.glob("src-cand-*")) if evidence.is_dir() else []:
        reason = clone_keep_reason(clone, accepted_repo, tip)
        if reason:
            kept.append(f"{clone}: {reason}")
        else:
            shutil.rmtree(clone)
            removed.append(clone)
    return removed, kept


# What `.local/evidence/<task>/` keeps once the task has landed: records, not build outputs.
RECORD_SUFFIXES = frozenset({".json", ".log", ".md", ".out", ".txt"})


def remove_build_outputs(root: Path, task_id: str) -> list[Path]:
    """Delete a landed task's rebuildable evidence outputs: its `gitturtle-*` executables, its package
    bundles, archives and extracts (`pkg-*`) and any leftover `.target-*` directory. Specs, attestations,
    logs and the source clones (see `remove_source_clones`) are not touched."""
    evidence = root / ".local/evidence" / task_id
    removed = []
    for path in sorted(evidence.iterdir()) if evidence.is_dir() else []:
        name, suffixes = path.name, set(path.suffixes)
        if path.is_symlink():
            continue
        if path.is_dir() and name.startswith(("pkg-", ".target-")):
            shutil.rmtree(path)
        elif path.is_file() and (name.startswith("pkg-") and name.endswith((".tar.gz", ".tar.gz.sha256"))
                                 or name.startswith("gitturtle-") and not suffixes & RECORD_SUFFIXES):
            path.unlink()
        else:
            continue
        removed.append(path)
    return removed


def land(args: argparse.Namespace) -> int:
    script_root = Path(__file__).resolve().parents[2]
    root = main_checkout(script_root)
    if not TASK_ID.fullmatch(args.task):
        raise LandError(f"{args.task!r} is not a task id")
    run = resolve_run(root, args.run)
    state = run_status(script_root, run)
    record = state["tasks"].get(args.task)
    if not isinstance(record, dict):
        raise LandError(f"{args.task} is not a task of run {run.name}")
    task = load_task(run, args.task)
    accepted_repo = run / "accepted"
    commits = select_commits(accepted_repo, record)
    say(f"{args.task}: {len(commits)} accepted commit(s), {record['base'][:12]}..{commits[-1][:12]}")
    allowed = {HANDOFF, str(state.get("spec_path", HANDOFF))}
    handoff = read_handoff(args.handoff_commit.expanduser(), allowed) if args.handoff_commit else None

    worktree = root / ".local/land" / args.task
    branch = BRANCH_PREFIX + args.task
    cleanup_hint = f"remove it with `git worktree remove --force {worktree}` and `git branch -D {branch}`"
    if worktree.exists() or worktree.is_symlink():
        raise LandError(f"{worktree} exists; inspect it, then {cleanup_hint}")
    if git_result(root, "show-ref", "--verify", "--quiet", f"refs/heads/{branch}").returncode == 0:
        raise LandError(f"the branch {branch} exists; inspect it, then `git branch -D {branch}`")
    git(root, "fetch", "--quiet", "origin", "main")
    main_sha = git(root, "rev-parse", "--verify", "refs/remotes/origin/main^{commit}").strip()
    already = landed_as(root, main_sha, task["commit"])
    if already:
        raise LandError(f"{task['commit']!r} is already on origin/main as {already[:12]}")
    earlier = earlier_landed(root, main_sha, run, state, args.task)
    if earlier:
        say("tasks accepted ahead of it are on origin/main: " + ", ".join(earlier))
    worktree.parent.mkdir(parents=True, exist_ok=True)
    git(root, "worktree", "add", "--quiet", "-b", branch, str(worktree), main_sha)
    say(f"worktree {worktree} on {branch} from origin/main {main_sha[:12]}")

    try:
        git(worktree, "fetch", "--quiet", "--no-tags", str(accepted_repo), "HEAD")
        copies, merged = [], []
        for commit in commits:
            if cherry_pick(worktree, commit):
                merged.append(commit)
            copies.append(head(worktree))
        problems = identity_problems(accepted_repo, commits, worktree, copies)
        if problems:
            raise LandError("patch identity differs:\n  " + "\n  ".join(problems))
        if handoff:
            check = git_result(worktree, "apply", "--check", str(handoff[0]))
            if check.returncode:
                raise LandError("the HANDOFF patch does not apply on the landed commits: "
                                + check.stderr.decode("utf-8", "replace").strip())
    except LandError as error:
        raise LandError(f"{error}\nThe worktree is left for inspection; {cleanup_hint}.") from None
    titles = [(commit, subject(accepted_repo, commit)) for commit in commits]
    for (commit, text), copy in zip(titles, copies):
        say(f"  {copy[:12]} <- {commit[:12]} {text}")
    say("patch identity matches the accepted range")
    for commit in merged:
        say(f"  {VALIDATION}: kept both entries, newest first, in the copy of {commit[:12]}")

    home = str(Path.home())
    body = pr_body(task, record, run_id=run.name, queue=Path(str(state.get("spec_path", "tasks.json"))).name,
                   commits=titles, attestations=read_attestations(run, record),
                   replacements=((f"{root}/", ""), (str(root), "."), (home, "~")))
    title = task["commit"]
    touches_workflows = any(path.startswith(".github/workflows/")
                            for copy in copies for path in changed_paths(worktree, f"{copy}^", copy))
    remote = push_target(worktree, touches_workflows)

    if args.dry_run:
        say("dry run: stopping after the patch-identity check. A real run would:")
        print(f"  - {'commit the HANDOFF patch ' + str(handoff[0]) if handoff else 'add no HANDOFF commit'}")
        print(f"  - push {branch} to {remote}, without force")
        print(f"  - open a PR into main titled: {title}")
        print("  - wait for its checks, `gh pr merge --squash --delete-branch --match-head-commit <pushed head>`")
        print(f"    when all pass, fast-forward {root} if it is clean and on main, remove the worktree, and remove")
        print("    each clean src-cand-* clone whose HEAD the landed range contains")
        print("PR body:\n" + body)
        remove_worktree(root, worktree, branch)
        say(f"removed the dry-run worktree and {branch}")
        return 0

    handoff_commit = None
    if handoff:
        handoff_commit = apply_handoff(worktree, handoff[0], handoff[1], args.task, allowed)
        say(f"HANDOFF commit {handoff_commit[:12]}")
    body_file = root / ".local/land" / f"{args.task}.pr-body.md"
    body_file.write_text(body, encoding="utf-8")
    pushed = head(worktree)
    git(worktree, "push", "--quiet", remote, f"{pushed}:refs/heads/{branch}")
    say(f"pushed {branch} at {pushed[:12]} to {remote}")
    created = gh(worktree, "pr", "create", "--base", "main", "--head", branch, "--title", title,
                 "--body-file", str(body_file))
    url = created.stdout.strip().splitlines()[-1] if created.stdout.strip() else ""
    match = re.search(r"/pull/([0-9]+)$", url)
    if not match:
        raise LandError(f"gh pr create printed no PR URL: {created.stdout.strip()} {created.stderr.strip()}")
    number = match[1]
    say(f"opened {url}")

    def finish(tip: str) -> str:
        # `--delete-branch` deletes the local branch too, which Git refuses while a worktree has it checked out.
        return (f"once its checks pass, `git worktree remove --force {worktree}` if it is still there, then "
                f"`gh pr merge {number} --squash --delete-branch --match-head-commit {tip}`, and fast-forward main")

    def merge_main_by_hand(start: str) -> str:
        # The same fetch land.py makes, so that the merge never takes a stale origin/main; the push target
        # depends on what that merge brings in.
        return (f"To finish by hand, {start}run `git fetch origin main` and `git merge origin/main` in {worktree}, "
                f"resolve, commit and push with {push_by_hand(root, branch, pushed, touches_workflows)}; "
                f"then, {finish('<that head>')}.")

    if args.no_merge:
        say(f"--no-merge: stopping here; {finish(pushed)}")
        return 0
    base, updates = main_sha, 0
    while True:
        try:
            count = wait_for_checks(worktree, number, pushed)
        except LandError as error:
            raise LandError(f"{error}\nPR #{number} stays open; after a fix or `gh run rerun --failed`, "
                            f"{finish(pushed)}.") from None
        say(f"all {count} checks passed on {pushed[:12]}")
        # The branch must be checked out nowhere for `--delete-branch` to delete it locally too.
        git(root, "worktree", "remove", str(worktree))
        try:
            # Merge exactly the head whose checks passed; GitHub refuses if the branch moved since.
            gh(root, "pr", "merge", number, "--squash", "--delete-branch", "--match-head-commit", pushed)
            break
        except LandError as error:
            refusal = str(error)
        # With strict required checks, a PR whose base moved while its checks ran is refused as behind.
        merge_state = pr_view(root, number, "mergeStateStatus,headRefOid")
        if merge_state.get("mergeStateStatus") != "BEHIND" and not BEHIND_REFUSAL.search(refusal):
            raise LandError(f"{refusal}\nPR #{number} stays open and {branch} stays; merge it by hand, "
                            "then fast-forward main")
        if merge_state.get("headRefOid") not in (None, pushed):
            raise LandError(f"PR #{number}'s head moved to {str(merge_state['headRefOid'])[:12]}, not the pushed "
                            f"{pushed[:12]}; it stays open and {branch} stays; merge it by hand, "
                            "then fast-forward main")
        if updates == MAX_BRANCH_UPDATES:
            raise LandError(f"{refusal}\nPR #{number} is still behind main after {MAX_BRANCH_UPDATES} updates "
                            f"and stays open at {pushed[:12]}. "
                            + merge_main_by_hand(f"`git worktree add {worktree} {branch}`, "))
        updates += 1
        say(f"PR #{number} is behind main, which moved while its checks ran: update {updates} of {MAX_BRANCH_UPDATES}")
        try:
            base, merged = update_branch(root, worktree, branch, pushed, base)
            problems = updated_problems(accepted_repo, commits, worktree, base, merged, handoff_commit, allowed)
            if problems:
                raise LandError(f"the change of {merged[:12]} against origin/main {base[:12]} no longer has the "
                                "accepted range's patch identity:\n  " + "\n  ".join(problems))
            say(f"the change against origin/main {base[:12]} still has the accepted range's patch identity")
            # A merge that brings main's workflow edits needs the `workflow` scope, like a task that edits them.
            remote = push_target(worktree, touches_workflows or any(
                path.startswith(".github/workflows/") for path in changed_paths(worktree, pushed, merged)))
            git(worktree, "push", "--quiet", remote, f"{merged}:refs/heads/{branch}")
        except MergeConflict as error:
            raise LandError(f"{error}\nNothing was pushed and PR #{number} stays open at {pushed[:12]}; {worktree} is "
                            f"left on {branch}. " + merge_main_by_hand("")) from None
        except LandError as error:
            raise LandError(f"{error}\nNothing was pushed and PR #{number} stays open at {pushed[:12]}; {worktree} "
                            f"is left for inspection; {cleanup_hint}.") from None
        pushed = merged
        say(f"pushed {branch} at {pushed[:12]} to {remote}")
    view = json.loads(gh(root, "pr", "view", number, "--json", "state,mergeCommit").stdout)
    if view.get("state") != "MERGED":
        raise LandError(f"PR #{number} is {view.get('state')} after the merge; {finish(pushed)}")
    merge_sha = (view.get("mergeCommit") or {}).get("oid", "")
    say(f"merged PR #{number} as {merge_sha[:12]}")
    remove_worktree(root, worktree, branch)

    git(root, "fetch", "--quiet", "origin", "main")
    current = git(root, "rev-parse", "--abbrev-ref", "HEAD").strip()
    dirty = git(root, "status", "--porcelain").strip()
    if current == "main" and not dirty:
        git(root, "merge", "--quiet", "--ff-only", "refs/remotes/origin/main")
        say(f"fast-forwarded {root} to {head(root)[:12]}")
    else:
        say(f"left {root} alone ({'on ' + current if current != 'main' else 'not clean'}); "
            "fast-forward it with `git merge --ff-only origin/main`")
    removed, kept = remove_source_clones(root, args.task, accepted_repo, commits[-1])
    for clone in removed:
        say(f"removed {clone}")
    for reason in kept:
        say(f"kept {reason}")
    for path in remove_build_outputs(root, args.task):
        say(f"removed {path}")
    body_file.unlink(missing_ok=True)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--run", required=True, help="run directory, or its name under .local/agent-loop")
    parser.add_argument("--task", required=True, help="the accepted task's id")
    handoff = parser.add_mutually_exclusive_group()
    handoff.add_argument("--handoff-commit", type=Path, metavar="PATCHFILE",
                         help="a `git diff` or `git format-patch` of HANDOFF.md (and the run's queue file) to commit last")
    handoff.add_argument("--no-handoff", action="store_true", help="land without a HANDOFF commit (the default)")
    parser.add_argument("--dry-run", action="store_true", help="stop after the patch-identity check")
    parser.add_argument("--no-merge", action="store_true", help="stop once the PR is open")
    args = parser.parse_args(argv)
    if sys.version_info < (3, 11):
        print("land: Python 3.11 or newer is required", file=sys.stderr)
        return 1
    try:
        return land(args)
    except LandError as error:
        print(f"land: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
