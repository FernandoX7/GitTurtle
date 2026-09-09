#!/usr/bin/env python3
"""Create disposable native recovery/rewrite QA worktrees and one local remote.

Usage: python3 scripts/create-recovery-rewrite-fixtures.py /tmp/unique-qa-directory
The destination must not exist. No user repository or global config is changed.
"""
import hashlib
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).absolute()
project = pathlib.Path(__file__).resolve().parent.parent
if root.exists() or root == project or project in root.parents:
    raise SystemExit("Choose a new destination outside the checkout")
root.mkdir(parents=True)

def git(repo, *args, succeeds=True):
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True)
    if succeeds and result.returncode:
        raise RuntimeError(result.stderr.decode(errors="replace"))
    if not succeeds and not result.returncode:
        raise RuntimeError("Expected a conflict or sequencer pause")
    return result.stdout.decode().strip()

def initialize(name, bare=False):
    repo = root / name
    repo.mkdir()
    git(repo, "init", *( ["--bare"] if bare else []), "-b", "main")
    git(repo, "config", "user.name", "GitTurtle Recovery QA")
    git(repo, "config", "user.email", "recovery-qa@example.invalid")
    git(repo, "config", "commit.gpgsign", "false")
    git(repo, "config", "core.hooksPath", "hooks" if bare else ".git/hooks")
    return repo

def commit(repo, name, data, message):
    (repo / name).write_bytes(data)
    git(repo, "add", "--", name)
    git(repo, "commit", "-m", message)
    return git(repo, "rev-parse", "HEAD")

conflict = initialize("conflict-drafts")
commit(conflict, "story.txt", b"A shared beginning\nA shared ending\n", "Shared story")
git(conflict, "switch", "-c", "incoming")
commit(conflict, "story.txt", b"Incoming beginning\nA shared ending\n", "Incoming story")
git(conflict, "switch", "main")
commit(conflict, "story.txt", b"Current beginning\nA shared ending\n", "Current story")
git(conflict, "merge", "incoming", succeeds=False)
(conflict / "unrelated.txt").write_bytes(b"Keep this staged work exactly\n")
git(conflict, "add", "unrelated.txt")
(conflict / "untracked.txt").write_bytes(b"Keep this untracked work\n")

rewrite = initialize("rewrite-series")
remote = initialize("rewrite-remote.git", bare=True)
base = commit(rewrite, "README.md", b"# Native rewrite fixture\n", "Base stays unchanged")
for number, message in [(1, "Add first note"), (2, "Add second note"), (3, "Add third note")]:
    commit(rewrite, f"note-{number}.txt", f"Note {number}\n".encode(), message)
original = git(rewrite, "rev-parse", "HEAD")
git(rewrite, "remote", "add", "origin", str(remote))
git(rewrite, "push", "-u", "origin", "main")
(rewrite / "untracked-keep.txt").write_bytes(b"Keep through rewrite and publish\n")

expected = {
    "conflict_worktree": str(conflict.resolve()),
    "conflict_file": "story.txt",
    "conflict_working_sha256": hashlib.sha256((conflict / "story.txt").read_bytes()).hexdigest(),
    "conflict_index_stages": git(conflict, "ls-files", "--stage"),
    "draft_text": "Recovered result 🐢\n\nKeep this exact trailing space  \n",
    "rebase_message_draft": "Reworded note 🐢\n\nKeep this message body exactly.  \n",
    "rewrite_worktree": str(rewrite.resolve()),
    "rewrite_base": base,
    "original_tip": original,
    "local_remote": str(remote.resolve()),
    "remote_branch": "refs/heads/main",
}
(root / "fixture.json").write_text(json.dumps(expected, indent=2, ensure_ascii=False) + "\n")
print(json.dumps(expected, indent=2, ensure_ascii=False))
