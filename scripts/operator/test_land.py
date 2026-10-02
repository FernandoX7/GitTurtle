"""Tests for land.py on temporary Git repositories: no network, and a stub `gh` stands in for GitHub.

Run: python3 -B -m unittest discover -s scripts/operator -p 'test_*.py'
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import land  # noqa: E402

GIT_ENV = {
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "Operator Test", "GIT_AUTHOR_EMAIL": "operator@example.invalid",
    "GIT_COMMITTER_NAME": "Operator Test", "GIT_COMMITTER_EMAIL": "operator@example.invalid",
}
# Each test's global Git configuration. Automatic maintenance stays off: since Git 2.47
# its detached child can still be writing a repository when the test removes it (see
# scripts/agent_loop/test_support.py). A file, unlike GIT_CONFIG_COUNT, also reaches
# the receive-pack of a local push, whose environment Git clears of that variable.
GIT_CONFIG = "[maintenance]\n auto = false\n[gc]\n auto = 0\n autoDetach = false\n"
GUIDE = "# Validation notes\n\nIntro.\n\n## Current validation guidance\n\nRows.\n\n"
OLD = "## October 1 an older entry\n\nTask `old`.\n\nNot covered natively: macOS.\n\n"
OLDEST = "## September 30 the oldest entry\n\nTask `oldest`.\n"


def entry(date: str, title: str, task: str) -> str:
    return f"## {date} {title}\n\nTask `{task}`. Frames: `docs/evidence/{task}/a.png`.\n\nNot covered natively: macOS.\n\n"


class GitCase(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        config = Path(directory.name) / "gitconfig"
        config.write_text(GIT_CONFIG, encoding="utf-8")
        environment = patch.dict(os.environ, GIT_ENV | {"GIT_CONFIG_GLOBAL": str(config)})
        environment.start()
        self.addCleanup(environment.stop)
        for name in land.FOREIGN_GIT:
            os.environ.pop(name, None)
        # Resolved, as Git and land.py report paths: macOS's temporary directory sits under the /var symlink.
        top = Path(directory.name).resolve()
        self.repo = top / "repo"
        self.git("init", "-q", "-b", "main", str(self.repo), cwd=top)

    def git(self, *args: str, cwd: Path | None = None) -> str:
        return subprocess.run(["git", *args], cwd=cwd or self.repo, check=True, capture_output=True,
                              text=True).stdout.strip()

    def commit(self, message: str, files: dict[str, str | bytes]) -> str:
        for name, content in files.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(content, bytes):
                path.write_bytes(content)
            else:
                path.write_text(content, encoding="utf-8")
        self.git("add", "--", *files)
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def accepted_chain(self) -> tuple[str, str, str]:
        """Base B, candidate C (code), evidence E (frame and dated entry) on `main`."""
        base = self.commit("chore: base", {"src/app.rs": "fn main() {}\n",
                                           land.VALIDATION: GUIDE + OLD + OLDEST})
        candidate = self.commit("fix(app): the task", {"src/app.rs": "fn main() { run(); }\n"})
        evidence = self.commit("docs(evidence): the task", {
            "docs/evidence/task/a.png": b"\x89PNG\x00candidate",
            land.VALIDATION: GUIDE + entry("October 2", "the task", "task") + OLD + OLDEST,
        })
        return base, candidate, evidence


class RangeSelectionTests(GitCase):
    def test_candidate_only_without_an_evidence_commit(self) -> None:
        base, candidate, _ = self.accepted_chain()
        record = {"status": "accepted", "base": base, "candidate": candidate}
        self.assertEqual(land.select_commits(self.repo, record), [candidate])

    def test_candidate_then_evidence_commit(self) -> None:
        base, candidate, evidence = self.accepted_chain()
        record = {"status": "accepted", "base": base, "candidate": candidate, "evidence_commit": evidence}
        self.assertEqual(land.select_commits(self.repo, record), [candidate, evidence])
        record["evidence_commit"] = {"commit": evidence}
        self.assertEqual(land.select_commits(self.repo, record), [candidate, evidence])

    def test_refuses_what_is_not_an_accepted_chain(self) -> None:
        base, candidate, evidence = self.accepted_chain()
        accepted = {"status": "accepted", "base": base, "candidate": candidate, "evidence_commit": evidence}
        cases = {
            "not accepted": {**accepted, "status": "awaiting_evidence"},
            "landed before the run": {**accepted, "landed": base},
            "no base": {**accepted, "base": None},
            "candidate outside the range": {**accepted, "candidate": base},
            "tip off the accepted checkout": {**accepted, "evidence_commit": "0" * 40},
        }
        for name, record in cases.items():
            with self.subTest(name), self.assertRaises(land.LandError):
                land.select_commits(self.repo, record)

    def test_refuses_an_evidence_commit_that_is_not_the_candidates_child(self) -> None:
        base, candidate, evidence = self.accepted_chain()
        extra = self.commit("docs(evidence): more", {"docs/evidence/task/b.png": b"\x89PNG\x00more"})
        record = {"status": "accepted", "base": base, "candidate": candidate, "evidence_commit": extra}
        with self.assertRaisesRegex(land.LandError, "not a child of the candidate"):
            land.select_commits(self.repo, record)
        self.assertNotEqual(evidence, extra)

    def test_refuses_a_merge_in_the_range(self) -> None:
        base, candidate, _ = self.accepted_chain()
        self.git("checkout", "-q", "-b", "side", base)
        self.commit("chore: side", {"side.txt": "side\n"})
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "--no-edit", "side")
        tip = self.git("rev-parse", "HEAD")
        record = {"status": "accepted", "base": base, "candidate": candidate, "evidence_commit": tip}
        with self.assertRaisesRegex(land.LandError, "linear chain"):
            land.select_commits(self.repo, record)


class ValidationMergeTests(unittest.TestCase):
    parent = GUIDE + OLD + OLDEST

    def merge(self, ours_entry: str, picked_entry: str) -> str:
        return land.merge_validation(GUIDE + ours_entry + OLD + OLDEST, self.parent,
                                     GUIDE + picked_entry + OLD + OLDEST)

    def test_picked_entry_older_than_mains(self) -> None:
        mine, picked = entry("October 3", "main's", "m"), entry("October 2", "picked", "p")
        self.assertEqual(self.merge(mine, picked), GUIDE + mine + picked + OLD + OLDEST)

    def test_picked_entry_newer_than_mains(self) -> None:
        mine, picked = entry("October 2", "main's", "m"), entry("October 3", "picked", "p")
        self.assertEqual(self.merge(mine, picked), GUIDE + picked + mine + OLD + OLDEST)

    def test_identical_heading_keeps_both_and_the_landing_entry_first(self) -> None:
        mine, picked = entry("October 2", "focus rings", "m"), entry("October 2", "focus rings", "p")
        self.assertEqual(self.merge(mine, picked), GUIDE + picked + mine + OLD + OLDEST)

    def test_new_year_orders_january_first(self) -> None:
        mine, picked = entry("December 31", "main's", "m"), entry("January 1", "picked", "p")
        self.assertEqual(self.merge(mine, picked), GUIDE + picked + mine + OLD + OLDEST)

    def test_keeps_mains_own_order_around_the_picked_entry(self) -> None:
        newest, older = entry("October 4", "newest", "n"), entry("October 2", "older", "o")
        picked = entry("October 3", "picked", "p")
        self.assertEqual(self.merge(newest + older, picked), GUIDE + newest + picked + older + OLD + OLDEST)

    def test_refuses_what_is_not_an_added_entry(self) -> None:
        mine = entry("October 3", "main's", "m")
        edited = OLD.replace("macOS", "macOS and Wayland")
        cases = {
            "edits an entry": GUIDE + entry("October 2", "picked", "p") + edited + OLDEST,
            "removes an entry": GUIDE + entry("October 2", "picked", "p") + OLD,
            "changes the preamble": GUIDE.replace("Intro", "Preface") + entry("October 2", "p", "p") + OLD + OLDEST,
            "adds an undated section": GUIDE + "## Notes\n\nText.\n\n" + OLD + OLDEST,
            "adds nothing": self.parent,
        }
        for name, picked in cases.items():
            with self.subTest(name), self.assertRaises(land.LandError):
                land.merge_validation(GUIDE + mine + OLD + OLDEST, self.parent, picked)

    def test_refuses_an_entry_already_on_the_branch(self) -> None:
        picked = entry("October 2", "picked", "p")
        with self.assertRaisesRegex(land.LandError, "already"):
            land.merge_validation(GUIDE + picked + OLD + OLDEST, self.parent, GUIDE + picked + OLD + OLDEST)

    def test_added_lines_tolerate_where_a_diff_puts_blank_lines(self) -> None:
        self.assertTrue(land.same_lines(["## A", "", "text", ""], ["", "## A", "", "text"]))
        self.assertFalse(land.same_lines(["## A", "text"], ["text", "## A"]))
        self.assertFalse(land.same_lines(["## A", "text"], ["## A", "text", ""]))
        diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n-old\n+--- kept\n+new\n"
        self.assertEqual(land.changed_lines(diff), (["--- kept", "new"], ["old"]))


class CherryPickAndIdentityTests(GitCase):
    def setUp(self) -> None:
        super().setUp()
        self.base, self.candidate, self.evidence = self.accepted_chain()
        self.git("checkout", "-q", "-b", "upstream", self.base)
        self.mine = entry("October 3", "main's entry", "m")
        self.commit("docs: main's entry", {land.VALIDATION: GUIDE + self.mine + OLD + OLDEST,
                                           "README.md": "readme\n"})
        self.git("checkout", "-q", "-b", "landing")

    def land_both(self) -> list[str]:
        copies = []
        merged = []
        for commit in (self.candidate, self.evidence):
            merged.append(land.cherry_pick(self.repo, commit))
            copies.append(land.head(self.repo))
        self.assertEqual(merged, [False, True])
        return copies

    def test_conflicting_entries_merge_newest_first_with_the_same_lines(self) -> None:
        copies = self.land_both()
        text = (self.repo / land.VALIDATION).read_text(encoding="utf-8")
        self.assertEqual(text, GUIDE + self.mine + entry("October 2", "the task", "task") + OLD + OLDEST)
        self.assertIn(f"(cherry picked from commit {self.evidence})", self.git("log", "-1", "--format=%B"))
        self.assertEqual(land.identity_problems(self.repo, [self.candidate, self.evidence], self.repo, copies), [])
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_identity_reports_a_changed_patch_validation_line_or_frame(self) -> None:
        copies = self.land_both()
        tip = copies[-1]
        merged = (self.repo / land.VALIDATION).read_text(encoding="utf-8")
        changes = {
            "another file": ({"src/app.rs": "fn main() { run(); stop(); }\n"}, ["also changes src/app.rs"]),
            "another entry text": ({land.VALIDATION: merged.replace("the task\n", "the task!\n")},
                                   [f"changes {land.VALIDATION} differently"]),
            "another frame": ({"docs/evidence/task/a.png": b"\x89PNG\x00other"},
                              ["changes docs/evidence/task/a.png differently"]),
        }
        for name, (files, expected) in changes.items():
            with self.subTest(name):
                self.git("checkout", "-q", "-B", "tampered", copies[0])
                self.commit("docs(evidence): the task", {
                    "docs/evidence/task/a.png": b"\x89PNG\x00candidate", land.VALIDATION: merged, **files})
                problems = land.identity_problems(self.repo, [self.candidate, self.evidence], self.repo,
                                                  [copies[0], land.head(self.repo)])
                self.assertEqual(len(problems), len(expected), problems)
                for problem, word in zip(problems, expected):
                    self.assertIn(word, problem)
        self.assertNotEqual(tip, copies[0])
        self.assertTrue(land.identity_problems(self.repo, [self.candidate, self.evidence], self.repo, copies[:1]))

    def test_a_nearby_edit_on_main_does_not_count_as_another_patch(self) -> None:
        lines = [f"line {number}\n" for number in range(12)]
        self.git("checkout", "-q", "-b", "nearby", self.base)
        before = self.commit("chore: longer file", {"src/long.rs": "".join(lines)})
        changed = lines[:6] + ["line 6 from the task\n"] + lines[7:]
        original = self.commit("fix(app): one line", {"src/long.rs": "".join(changed)})
        self.git("checkout", "-q", "-b", "nearby-main", before)
        self.commit("chore: a line two above", {"src/long.rs": "".join(lines[:4] + ["line 4 on main\n"] + lines[5:])})
        self.assertFalse(land.cherry_pick(self.repo, original))
        copy = land.head(self.repo)
        self.assertEqual(land.identity_problems(self.repo, [original], self.repo, [copy]), [])
        with_context = [self.git("diff-tree", "-p", f"{commit}^", commit) for commit in (original, copy)]
        self.assertNotEqual(with_context[0].split("@@", 1)[1], with_context[1].split("@@", 1)[1])

    def test_another_conflict_aborts_and_leaves_the_branch_as_it_was(self) -> None:
        self.commit("fix(app): another change", {"src/app.rs": "fn main() { other(); }\n"})
        before = land.head(self.repo)
        with self.assertRaisesRegex(land.LandError, "src/app.rs"):
            land.cherry_pick(self.repo, self.candidate)
        self.assertEqual(land.head(self.repo), before)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_the_update_check_fails_closed_when_the_range_and_the_handoff_edit_one_path(self) -> None:
        bookkeeping = {land.HANDOFF, "docs/development/queue.json"}
        self.git("checkout", "-q", "-B", "bookkeeping", self.base)
        original = self.commit("fix(app): the task", {land.HANDOFF: "a\nfrom the task\n",
                                                      "src/app.rs": "fn main() { run(); }\n"})
        handoff = self.commit("docs(dev): record the landing", {"docs/development/queue.json": "{}\n"})
        self.assertEqual(land.updated_problems(self.repo, [original], self.repo, self.base, handoff, handoff,
                                               bookkeeping), [])
        self.git("checkout", "-q", "-B", "bookkeeping", original)
        handoff = self.commit("docs(dev): record the landing", {land.HANDOFF: "a\nfrom the task\nlanded\n"})
        self.assertEqual(land.updated_problems(self.repo, [original], self.repo, self.base, handoff, handoff,
                                               bookkeeping),
                         [f"the accepted range and the HANDOFF commit {handoff[:12]} both edit {land.HANDOFF}, "
                          "which this check cannot separate; land it by hand"])

    def test_landed_as_finds_a_squashed_subject(self) -> None:
        self.commit("fix(app): the task (#12)", {"x.txt": "x\n"})
        self.assertEqual(land.landed_as(self.repo, "HEAD", "fix(app): the task"), land.head(self.repo))
        self.assertIsNone(land.landed_as(self.repo, "HEAD", "fix(app): the tas"))


class EarlierTasksTests(GitCase):
    """Task `b` sits on task `a`'s evidence commit; `a` has to be on origin/main first."""

    def setUp(self) -> None:
        super().setUp()
        self.base, self.candidate, self.evidence = self.accepted_chain()
        self.later = self.commit("fix(app): task b", {"src/b.rs": "fn b() {}\n"})
        self.state = {"source_head": self.base, "spec_path": "docs/development/queue.json", "tasks": {
            "a": {"status": "accepted", "base": self.base, "candidate": self.candidate,
                  "evidence_commit": self.evidence},
            "b": {"status": "accepted", "base": self.evidence, "candidate": self.later},
            "c": {"status": "pending"},
        }}
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.run_dir = Path(directory.name)
        self.git("clone", "-q", "--no-local", str(self.repo), str(self.run_dir / "accepted"))
        (self.run_dir / "tasks.json").write_text(json.dumps({"version": 1, "tasks": [
            {"id": "a", "title": "Task a", "commit": "fix(app): the task"},
            {"id": "b", "title": "Task b", "commit": "fix(app): task b"}]}), encoding="utf-8")

    def upstream(self, subject: str, tree: str | None, handoff: bool = True) -> str:
        """origin/main: the base, then one squash with `tree`'s content (and a HANDOFF edit)."""
        self.git("checkout", "-q", "-B", "upstream", self.base)
        if tree:
            self.git("checkout", tree, "--", ".")
        if handoff:
            (self.repo / land.HANDOFF).parent.mkdir(parents=True, exist_ok=True)
            (self.repo / land.HANDOFF).write_text("handoff\n", encoding="utf-8")
            self.git("add", land.HANDOFF)
        self.git("commit", "-q", "--allow-empty", "-m", subject)
        return land.head(self.repo)

    def test_ranges_below_the_base_belong_to_accepted_tasks(self) -> None:
        self.assertEqual(land.earlier_ranges(self.repo, self.state, "b"), [("a", [self.candidate, self.evidence])])
        self.assertEqual(land.earlier_ranges(self.repo, self.state, "a"), [])
        del self.state["tasks"]["a"]["evidence_commit"]
        with self.assertRaisesRegex(land.LandError, "belongs to no accepted task"):
            land.earlier_ranges(self.repo, self.state, "b")

    def test_an_earlier_task_counts_only_with_its_subject_and_whole_range(self) -> None:
        tip = self.upstream("fix(app): the task (#7)", self.evidence)
        self.assertEqual(land.earlier_landed(self.repo, tip, self.run_dir, self.state, "b"), [f"a as {tip[:12]}"])
        tip = self.upstream("chore: something else", self.evidence)
        with self.assertRaisesRegex(land.LandError, "is not on origin/main"):
            land.earlier_landed(self.repo, tip, self.run_dir, self.state, "b")
        tip = self.upstream("fix(app): the task (#7)", self.candidate)
        with self.assertRaisesRegex(land.LandError, "not its accepted range.*misses"):
            land.earlier_landed(self.repo, tip, self.run_dir, self.state, "b")


class PushByHandTests(GitCase):
    def test_a_merge_by_hand_pushes_by_push_targets_rule(self) -> None:
        self.git("remote", "add", "origin", "https://github.com/owner/repo.git")
        since = "a" * 40
        self.assertEqual(land.push_by_hand(self.repo, "claude/land-task", since, False),
                         "`git push origin HEAD:claude/land-task`, or `git push git@github.com:owner/repo.git "
                         "HEAD:claude/land-task` if `git diff --name-only aaaaaaaaaaaa HEAD -- .github/workflows` "
                         "lists a file")
        self.assertEqual(land.push_by_hand(self.repo, "claude/land-task", since, True),
                         "`git push git@github.com:owner/repo.git HEAD:claude/land-task`")
        self.git("remote", "set-url", "origin", "git@github.com:owner/repo.git")
        self.assertEqual(land.push_by_hand(self.repo, "claude/land-task", since, True),
                         "`git push origin HEAD:claude/land-task`")


class CloneCleanupTests(GitCase):
    def test_only_clean_clones_inside_the_landed_range_go(self) -> None:
        base, candidate, evidence = self.accepted_chain()
        evidence_dir = self.repo.parent / "root/.local/evidence/task"
        clones = {name: evidence_dir / f"src-cand-{name}" for name in ("clean", "untracked", "ahead", "plain")}
        for name in ("clean", "untracked", "ahead"):
            self.git("clone", "-q", "--no-local", str(self.repo), str(clones[name]))
            self.git("checkout", "-q", "--detach", candidate, cwd=clones[name])
        (clones["untracked"] / "attestation.json").write_text("{}", encoding="utf-8")
        (clones["ahead"] / "extra.txt").write_text("extra\n", encoding="utf-8")
        self.git("add", "extra.txt", cwd=clones["ahead"])
        self.git("commit", "-q", "-m", "chore: unlanded", cwd=clones["ahead"])
        clones["plain"].mkdir()
        removed, kept = land.remove_source_clones(self.repo.parent / "root", "task", self.repo, evidence)
        self.assertEqual(removed, [clones["clean"]])
        self.assertFalse(clones["clean"].exists())
        self.assertEqual([line.split(":")[0] for line in kept],
                         [str(clones[name]) for name in ("ahead", "plain", "untracked")])
        self.assertTrue(all(clones[name].exists() for name in ("ahead", "plain", "untracked")))
        self.assertIn("local changes", " ".join(kept))
        self.assertNotEqual(base, evidence)


class LandingCase(GitCase):
    """A main checkout cloned from a local bare origin whose main gained an entry after the run's base,
    and a run whose task `task` is accepted, read through a stub `agent-loop.py status`."""

    def make_landing(self) -> tuple[Path, Path, Path, dict]:
        """(top directory, main checkout, run directory, the task's record)."""
        top = self.repo.parent
        base, candidate, evidence = self.accepted_chain()
        self.git("init", "-q", "--bare", "-b", "main", str(top / "origin.git"), cwd=top)
        self.git("push", "-q", str(top / "origin.git"), f"{base}:refs/heads/main")
        root = top / "root"
        self.git("clone", "-q", str(top / "origin.git"), str(root), cwd=top)
        self.git("commit", "-q", "--allow-empty", "-m", "chore: unrelated", cwd=root)
        (root / land.VALIDATION).write_text(GUIDE + entry("October 3", "main's", "m") + OLD + OLDEST,
                                            encoding="utf-8")
        self.git("commit", "-q", "-am", "docs: main's entry", cwd=root)
        self.git("push", "-q", "origin", "HEAD:main", cwd=root)
        self.git("reset", "-q", "--hard", base, cwd=root)  # the fetch has to bring origin/main up to date
        (root / "scripts/operator").mkdir(parents=True)
        (root / "scripts/operator/land.py").write_bytes(Path(land.__file__).read_bytes())
        (root / "scripts/agent-loop.py").write_text(
            "import sys\nrun = sys.argv[sys.argv.index('--run') + 1]\nprint(open(run + '/state.json').read())\n",
            encoding="utf-8")

        run = root / ".local/agent-loop/20261002T000000Z-abcd"
        run.mkdir(parents=True)
        self.git("clone", "-q", "--no-local", str(self.repo), str(run / "accepted"), cwd=top)
        record = {"status": "accepted", "base": base, "candidate": candidate, "evidence_commit": evidence,
                  "required_evidence": ["native"], "attestations": {}}
        (run / "state.json").write_text(json.dumps({"spec_path": "docs/development/queue.json", "source_head": base,
                                                    "tasks": {"task": record}}), encoding="utf-8")
        (run / "tasks.json").write_text(json.dumps({"version": 1, "tasks": [
            {"id": "task", "title": "Do the task", "commit": "fix(app): the task"}]}), encoding="utf-8")
        return top, root, run, record


class DryRunTests(LandingCase):
    """Steps 1 to 5 end to end against a local bare origin and a stub `agent-loop.py status`."""

    def test_dry_run_lands_merges_checks_and_cleans_up(self) -> None:
        top, root, run, record = self.make_landing()
        evidence = record["evidence_commit"]
        result = subprocess.run([sys.executable, "-B", str(root / "scripts/operator/land.py"), "--run", run.name,
                                 "--task", "task", "--dry-run"], cwd=top, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        for expected in ("2 accepted commit(s)", "patch identity matches the accepted range",
                         "kept both entries, newest first", "push claude/land-task to origin",
                         "titled: fix(app): the task", "evidence commit `" + evidence[:12]):
            self.assertIn(expected, result.stdout)
        self.assertFalse((root / ".local/land/task").exists())
        self.assertEqual(self.git("branch", "--list", "claude/land-task", cwd=root), "")

        record["status"] = "failed"
        (run / "state.json").write_text(json.dumps({"tasks": {"task": record}}), encoding="utf-8")
        result = subprocess.run([sys.executable, "-B", str(root / "scripts/operator/land.py"), "--run", str(run),
                                 "--task", "task", "--dry-run"], cwd=top, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("not 'accepted'", result.stderr)


# GitHub as the stub `gh` sees it: the PR is `branch` on the bare `origin`, every check passes at once, and
# each `pr checks` call first moves origin's main to the next of `moves`, as if another PR merged meanwhile.
# Like `main`'s strict required check, `pr merge` refuses a head whose branch lacks origin's main.
STUB_GH = r'''
import json, os, subprocess, sys
path = os.environ["STUB_GH_STATE"]
with open(path, encoding="utf-8") as handle:
    state = json.load(handle)
args = sys.argv[1:]
state["calls"].append(args)

def origin(*command, check=True):
    return subprocess.run(["git", "--git-dir", state["origin"], *command], check=check, capture_output=True,
                          text=True)

def branch_head():
    found = origin("rev-parse", "--verify", "--quiet", "refs/heads/" + state["branch"], check=False)
    return found.stdout.strip() or None

def behind():
    return origin("merge-base", "--is-ancestor", "refs/heads/main", "refs/heads/" + state["branch"],
                  check=False).returncode != 0

out, err, code = "", "", 0
if args[:2] == ["pr", "create"]:
    state["branch"] = args[args.index("--head") + 1]
    out = "https://github.com/owner/repo/pull/155\n"
elif args[:2] == ["pr", "checks"]:
    if state["moves"]:
        origin("update-ref", "refs/heads/main", state["moves"].pop(0))
    out = json.dumps([{"name": "Quality gate", "bucket": "pass", "link": ""}])
elif args[:2] == ["run", "list"]:
    out = json.dumps([{"status": "completed", "conclusion": "success", "workflowName": "Quality"}])
elif args[:2] == ["pr", "view"]:
    view = {"state": state["state"], "headRefOid": branch_head(), "mergeCommit": {"oid": state.get("merged", "")},
            "mergeStateStatus": "BEHIND" if state["state"] == "OPEN" and behind() else "CLEAN"}
    out = json.dumps({key: view[key] for key in args[args.index("--json") + 1].split(",")})
elif args[:2] == ["pr", "merge"]:
    tip = branch_head()
    if args[args.index("--match-head-commit") + 1] != tip:
        err, code = "the head moved", 1
    elif behind():
        err, code = ("X Pull request owner/repo#155 is not mergeable: the head branch is not up to date "
                     "with the base branch."), 1
    else:
        squash = origin("commit-tree", tip + "^{tree}", "-p", "refs/heads/main", "-m", "squash").stdout.strip()
        origin("update-ref", "refs/heads/main", squash)
        origin("update-ref", "-d", "refs/heads/" + state["branch"])
        subprocess.run(["git", "branch", "-D", state["branch"]], check=True, capture_output=True)
        state["state"], state["merged"] = "MERGED", squash
else:
    err, code = "the stub has no answer for gh " + " ".join(args), 2
with open(path, "w", encoding="utf-8") as handle:
    json.dump(state, handle)
sys.stdout.write(out)
sys.stderr.write(err)
sys.exit(code)
'''


class BranchUpdateTests(LandingCase):
    """Step 8 when another PR merges into main while this one's checks run (main requires strict checks)."""

    def setUp(self) -> None:
        super().setUp()
        self.top, self.root, self.run_dir, self.record = self.make_landing()
        # The stub scripts and run directory are untracked; excluded, the checkout counts as clean.
        (self.root / ".git/info/exclude").write_text("/scripts/\n/.local/\n", encoding="utf-8")
        self.origin = self.top / "origin.git"
        stub = self.top / "bin/gh"
        stub.parent.mkdir()
        stub.write_text(f"#!{sys.executable}\n{STUB_GH}", encoding="utf-8")
        stub.chmod(0o755)
        self.gh_state = self.top / "gh-state.json"

    def other_merges(self, *changes: dict[str, str]) -> list[str]:
        """Commits other PRs put on origin's main, one per wait for checks, each on the previous one."""
        other = self.top / "other"
        self.git("clone", "-q", str(self.origin), str(other), cwd=self.top)
        moves = []
        for number, files in enumerate(changes, 1):
            for name, content in files.items():
                (other / name).parent.mkdir(parents=True, exist_ok=True)
                (other / name).write_text(content, encoding="utf-8")
            self.git("add", "--", *files, cwd=other)
            self.git("commit", "-q", "-m", f"chore: another pull request {number}", cwd=other)
            moves.append(self.git("rev-parse", "HEAD", cwd=other))
        if moves:
            self.git("push", "-q", "origin", "HEAD:refs/heads/other", cwd=other)  # the objects reach origin
        return moves

    def land(self, moves: list[str], *options: str) -> tuple[subprocess.CompletedProcess, list[list[str]]]:
        self.gh_state.write_text(json.dumps({"origin": str(self.origin), "moves": moves, "state": "OPEN",
                                             "branch": "", "calls": []}), encoding="utf-8")
        environment = {**os.environ, "STUB_GH_STATE": str(self.gh_state),
                       "PATH": f"{self.top / 'bin'}{os.pathsep}{os.environ['PATH']}"}
        result = subprocess.run([sys.executable, "-B", str(self.root / "scripts/operator/land.py"), "--run",
                                 self.run_dir.name, "--task", "task", *options], cwd=self.top, capture_output=True,
                                text=True, env=environment)
        return result, json.loads(self.gh_state.read_text(encoding="utf-8"))["calls"]

    def origin_ref(self, name: str) -> str:
        return self.git("--git-dir", str(self.origin), "rev-parse", "--verify", "--quiet", name, cwd=self.top)

    def merged_heads(self, calls: list[list[str]]) -> list[str]:
        """The head each `gh pr merge` asked for."""
        return [call[call.index("--match-head-commit") + 1] for call in calls if call[:2] == ["pr", "merge"]]

    def test_behind_main_merges_it_in_waits_for_checks_and_lands_that_head(self) -> None:
        moves = self.other_merges({"other.txt": "another PR\n"})
        patch_file = self.top / "handoff.patch"
        patch_file.write_text(f"diff --git a/{land.HANDOFF} b/{land.HANDOFF}\nnew file mode 100644\n--- /dev/null\n"
                              f"+++ b/{land.HANDOFF}\n@@ -0,0 +1 @@\n+Landed the task.\n", encoding="utf-8")
        result, calls = self.land(moves, "--handoff-commit", str(patch_file))
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        first, updated = self.merged_heads(calls)
        parents = self.git("rev-list", "--parents", "-n", "1", updated, cwd=self.root).split()[1:]
        self.assertEqual(parents, [first, moves[0]])
        for expected in ("is behind main, which moved while its checks ran: update 1 of 3",
                         f"merged origin/main {moves[0][:12]} into claude/land-task as {updated[:12]}",
                         f"against origin/main {moves[0][:12]} still has the accepted range's patch identity",
                         f"pushed claude/land-task at {updated[:12]}", f"all 1 checks passed on {updated[:12]}",
                         "merged PR #155"):
            self.assertIn(expected, result.stdout)
        # The squash carries the task, the other PR and the HANDOFF commit, and the checkout followed it.
        squash = self.origin_ref("refs/heads/main")
        self.assertEqual(self.git("rev-parse", f"{squash}^", cwd=self.root), moves[0])
        for name, text in (("src/app.rs", "fn main() { run(); }"), ("other.txt", "another PR"),
                           (land.HANDOFF, "Landed the task.")):
            self.assertEqual(self.git("show", f"{squash}:{name}", cwd=self.root), text)
        self.assertEqual(land.head(self.root), squash)
        self.assertFalse((self.root / ".local/land/task").exists())
        self.assertEqual(self.git("branch", "--list", "claude/land-task", cwd=self.root), "")

    # Main's validation entry since the run's base (see `make_landing`), and another PR's entry of the task's date.
    mains_entry = entry("October 3", "main's", "m")
    others_entry = entry("October 2", "another task", "other")

    def test_a_conflict_only_in_validation_keeps_both_entries_and_lands(self) -> None:
        moves = self.other_merges({land.VALIDATION: GUIDE + self.mains_entry + self.others_entry + OLD + OLDEST})
        result, calls = self.land(moves)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        first, updated = self.merged_heads(calls)
        self.assertEqual(self.git("rev-list", "--parents", "-n", "1", updated, cwd=self.root).split()[1:],
                         [first, moves[0]])
        for expected in (f"merged origin/main {moves[0][:12]} into claude/land-task as {updated[:12]}",
                         f"{land.VALIDATION}: kept both sides' entries, newest first",
                         "still has the accepted range's patch identity", "merged PR #155"):
            self.assertIn(expected, result.stdout)
        # Both PRs' entries, the landing one first on their shared date, with their blank separators.
        squash = self.origin_ref("refs/heads/main")
        self.assertEqual(self.git("show", f"{squash}:{land.VALIDATION}", cwd=self.root) + "\n",
                         GUIDE + self.mains_entry + entry("October 2", "the task", "task") + self.others_entry
                         + OLD + OLDEST)

    def test_a_conflict_with_main_stops_with_the_commands_to_finish_by_hand(self) -> None:
        # The validation entries conflict too, but the two-entry merge only ever resolves them alone.
        moves = self.other_merges({"src/app.rs": "fn main() { other(); }\n",
                                   land.VALIDATION: GUIDE + self.mains_entry + self.others_entry + OLD + OLDEST})
        result, calls = self.land(moves)
        self.assertEqual(result.returncode, 1, result.stdout)
        worktree = self.root / ".local/land/task"
        pushed = self.merged_heads(calls)[0]
        for expected in (f"merging origin/main {moves[0][:12]} into claude/land-task stopped with conflicts in "
                         f"{land.VALIDATION}, src/app.rs", f"PR #155 stays open at {pushed[:12]}",
                         f"run `git fetch origin main` and `git merge origin/main` in {worktree}",
                         "push with `git push origin HEAD:claude/land-task`; then,",
                         f"`git worktree remove --force {worktree}` if it is still there, then "
                         "`gh pr merge 155 --squash --delete-branch --match-head-commit <that head>`"):
            self.assertIn(expected, result.stderr)
        # The merge was aborted: the worktree is clean at the pushed head, and nothing else was pushed.
        self.assertEqual(land.head(worktree), pushed)
        self.assertEqual(self.git("status", "--porcelain", cwd=worktree), "")
        self.assertEqual(self.origin_ref("refs/heads/claude/land-task"), pushed)
        self.assertEqual(len(self.merged_heads(calls)), 1)

    def test_an_update_whose_change_differs_from_the_accepted_range_is_not_pushed(self) -> None:
        # Another PR made the candidate's own change, so the merge is clean but the PR would no longer carry it.
        moves = self.other_merges({"src/app.rs": "fn main() { run(); }\n"})
        result, calls = self.land(moves)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("no longer has the accepted range's patch identity", result.stderr)
        self.assertIn("misses src/app.rs", result.stderr)
        self.assertIn("Nothing was pushed", result.stderr)
        pushed = self.merged_heads(calls)[0]
        self.assertEqual(self.origin_ref("refs/heads/claude/land-task"), pushed)
        self.assertEqual(self.git("rev-list", "--parents", "-n", "1", "HEAD", cwd=self.root / ".local/land/task")
                         .split()[1:], [pushed, moves[0]])

    def test_updates_stop_after_the_bound(self) -> None:
        moves = self.other_merges(*({f"other-{number}.txt": f"PR {number}\n"} for number in range(4)))
        result, calls = self.land(moves)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(f"still behind main after {land.MAX_BRANCH_UPDATES} updates", result.stderr)
        worktree = self.root / ".local/land/task"
        self.assertIn(f"`git worktree add {worktree} claude/land-task`, run `git fetch origin main` and "
                      f"`git merge origin/main` in {worktree}, resolve, commit and push with "
                      "`git push origin HEAD:claude/land-task`; then,", result.stderr)
        heads = self.merged_heads(calls)
        self.assertEqual(len(heads), land.MAX_BRANCH_UPDATES + 1)
        self.assertEqual(len(set(heads)), len(heads))
        self.assertEqual(heads[-1], self.origin_ref("refs/heads/claude/land-task"))
        self.assertEqual(result.stdout.count("is behind main"), land.MAX_BRANCH_UPDATES)
        self.assertNotIn("merged PR", result.stdout)


class HandoffTests(unittest.TestCase):
    def test_patch_may_touch_only_the_bookkeeping(self) -> None:
        allowed = {land.HANDOFF, "docs/development/queue.json"}
        good = f"diff --git a/{land.HANDOFF} b/{land.HANDOFF}\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"
        bad = good + "diff --git a/DESIGN.md b/DESIGN.md\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "handoff.patch"
            path.write_text(good, encoding="utf-8")
            self.assertEqual(land.read_handoff(path, allowed)[1], good)
            path.write_text(bad, encoding="utf-8")
            with self.assertRaisesRegex(land.LandError, "DESIGN.md"):
                land.read_handoff(path, allowed)
        self.assertTrue(land.is_mailbox("From abc Mon Sep 17 00:00:00 2001\nSubject: [PATCH] x\n\nbody"))
        self.assertFalse(land.is_mailbox(good))


class PrBodyTests(unittest.TestCase):
    def test_body_names_identity_evidence_and_no_local_paths(self) -> None:
        candidate, evidence, base = "c" * 40, "e" * 40, "b" * 40
        task = {"id": "tab-reveals", "title": "Reveal tab-focused rows", "commit": "fix(focus): reveal rows"}
        record = {"status": "accepted", "base": base, "candidate": candidate, "evidence_commit": evidence,
                  "accepted_at": "2026-10-02T00:25:22.5+00:00", "security_required": True,
                  "required_evidence": ["native"]}
        document = {
            "task": "tab-reveals", "candidate": candidate, "base": base, "evidence_commit": evidence,
            "attested_executable": {"path": "/home/u/repo/.local/evidence/x/gitturtle-cand", "sha256": "9ad93bd5" * 8,
                                    "source_revision": candidate, "source_tree": "clean", "profile": "release"},
            "host": "Ubuntu 26.04, GNOME 50, XWayland :0", "input": "Mutter RemoteDesktop",
            "sessions": [{"utc": "2026-10-01 23:24-23:57", "what": "base vs candidate in /home/u/repo/docs"}],
            "committed_frames": "docs/evidence/tab-reveals/ (20 crops)", "bundle": "/tmp/runs/tab-reveals",
            "limitations": "macOS not covered", "analyses": {"total": 36, "as_expected": 36},
        }
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "20261002T000000Z-abcd"
            (run / "attestations").mkdir(parents=True)
            (run / "attestations/native.evidence").write_text(json.dumps(document), encoding="utf-8")
            (run / "attestations/elsewhere.evidence").symlink_to(run / "attestations/native.evidence")
            record["attestations"] = {
                "native": {"candidate": candidate, "summary": "Re-captured byte-identical",
                           "artifact": "attestations/native.evidence"},
                "performance": {"candidate": candidate, "summary": "Measured", "artifact": "attestations/elsewhere.evidence"},
            }
            attestations = land.read_attestations(run, record)
        self.assertEqual([(kind, document is not None) for kind, _, document in attestations],
                         [("native", True), ("performance", False)])
        body = land.pr_body(task, record, run_id=run.name, queue="queue-2026-10-03.json",
                            commits=[(candidate, "fix(focus): reveal rows"), (evidence, "docs(evidence): rows")],
                            attestations=attestations, replacements=(("/home/u/repo/", ""), ("/home/u", "~")))
        for expected in ("Reveal tab-focused rows.", "`tab-reveals` of `queue-2026-10-03.json`", run.name,
                         f"candidate `{candidate[:12]}`", f"evidence commit `{evidence[:12]}`",
                         "the security review and the required evidence (`native`) passed", "9ad93bd5" * 8,
                         "(release build of `cccccccccccc`, source tree clean)", "2026-10-01 23:24-23:57",
                         ". The controller's gates, the independent verifier, ",
                         "base vs candidate in docs", "Limitations: macOS not covered", "### Performance",
                         "- Analyses: 36 of 36 as expected",
                         "- `docs(evidence): rows` (`eeeeeeeeeeee`)", "at 2026-10-02 00:25 UTC"):
            self.assertIn(expected, body)
        for absent in ("/home/u", "/tmp/runs", ".local/evidence"):
            self.assertNotIn(absent, body)


if __name__ == "__main__":
    unittest.main()
