"""Tests for land.py's pure parts on temporary Git repositories: no network, no gh.

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
    "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_AUTHOR_NAME": "Operator Test", "GIT_AUTHOR_EMAIL": "operator@example.invalid",
    "GIT_COMMITTER_NAME": "Operator Test", "GIT_COMMITTER_EMAIL": "operator@example.invalid",
}
GUIDE = "# Validation notes\n\nIntro.\n\n## Current validation guidance\n\nRows.\n\n"
OLD = "## October 1 an older entry\n\nTask `old`.\n\nNot covered natively: macOS.\n\n"
OLDEST = "## September 30 the oldest entry\n\nTask `oldest`.\n"


def entry(date: str, title: str, task: str) -> str:
    return f"## {date} {title}\n\nTask `{task}`. Frames: `docs/evidence/{task}/a.png`.\n\nNot covered natively: macOS.\n\n"


class GitCase(unittest.TestCase):
    def setUp(self) -> None:
        environment = patch.dict(os.environ, GIT_ENV)
        environment.start()
        self.addCleanup(environment.stop)
        for name in land.FOREIGN_GIT:
            os.environ.pop(name, None)
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name) / "repo"
        self.git("init", "-q", "-b", "main", str(self.repo), cwd=Path(directory.name))

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


class DryRunTests(GitCase):
    """Steps 1 to 5 end to end against a local bare origin and a stub `agent-loop.py status`."""

    def test_dry_run_lands_merges_checks_and_cleans_up(self) -> None:
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
