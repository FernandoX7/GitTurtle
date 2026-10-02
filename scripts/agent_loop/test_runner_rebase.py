"""Mechanical rebase of pending candidates, and implementing while candidates await evidence."""

from __future__ import annotations

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import shutil
import time
import unittest
from unittest.mock import patch

from agent_loop import rebase, test_runner as fixtures
from agent_loop.git import clean, git, head
from agent_loop.process import EnvironmentBlocked, LoopError, atomic_json, read_json
from agent_loop.rebase import stack_insertions
from agent_loop.runner import (
    IDLE_POLL, IDLE_TICK, MAX_AWAITING_EVIDENCE, MAX_IDLE_MINUTES, Runner, attest, create_run, locked, main, recheck_wanted,
)
from agent_loop.test_evidence_commit import FRAME
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


class SharedFileCodex(fixtures.FakeCodex):
    """Also writes `docs/shared.md` with the text its task maps to, so two candidates can overlap."""

    def __init__(self, shared, **options):
        super().__init__(script=True, **options)
        self.shared = shared

    def run(self, role, feature, repo, directory, timeout, stop, **options):
        result = super().run(role, feature, repo, directory, timeout, stop, **options)
        if role == "implementer" and feature.id in self.shared:
            (repo / "docs/shared.md").write_text(self.shared[feature.id])
        return result


LOG = "docs/validation.md"
GUIDANCE = "# Validation notes\n\n## Current validation guidance\n\nThe dated records apply to their named builds.\n\n"
OLDER = "## October 1 an older entry\n\nOld entry.\n"


def entry(task_id):
    """A dated entry as an evidence commit adds it: newest first, below the guidance."""
    return f"## October 2 {task_id}\n\nTask `{task_id}`: `docs/evidence/{task_id}/focus.png` shows the focused row.\n\n"


class LoggingCodex(fixtures.FakeCodex):
    """`two` also writes the validation log."""

    def __init__(self, *, log, **options):
        super().__init__(**options)
        self.log = log

    def run(self, role, feature, repo, directory, timeout, stop, **options):
        result = super().run(role, feature, repo, directory, timeout, stop, **options)
        if role == "implementer" and feature.id == "two":
            (repo / LOG).write_text(self.log)
        return result


def shared_tasks():
    """`one` waits for native evidence; `two`, a docs task, is accepted first. Both may write `docs/shared.md`."""
    one, two = fixtures.evidence_task("one"), fixtures.task("two")
    for contract in (one, two):
        contract["scope"].append("docs/shared.md")
    return [one, two]


class RebaseTests(unittest.TestCase):
    prepare = fixtures.RunnerTests.prepare
    create = fixtures.RunnerTests.create
    execute = fixtures.RunnerTests.execute

    def setUp(self):
        fixtures.RunnerTests.setUp(self)
        # A held lock is a running loop at once, so the operator's requests queue in its inbox.
        waiting = patch("agent_loop.runner.LOCK_WAIT", 0.0)
        waiting.start()
        self.addCleanup(waiting.stop)
        self.sleeps = []

    def run_loop(self, directory, adapter, gate=fixtures.green_gates, operator=None):
        """Run the loop as a controller holding the run lock, with an idle wait that costs no real time.

        Each tick of the wait advances a fake clock by what the loop asked to
        sleep, after `operator(runner)` had its turn, as an operator would act
        while the loop waits.
        """
        runner = Runner(directory, adapter=adapter, gate_runner=gate)
        clock = [0.0]
        runner.idle_clock = lambda: clock[0]

        def sleep(seconds):
            self.sleeps.append(seconds)
            if operator:
                operator(runner)
            clock[0] += seconds

        runner.idle_sleep = sleep
        output = io.StringIO()
        with locked(directory), redirect_stdout(output):
            state = runner.execute()
        self.output = output.getvalue()
        self.runner = runner
        return state

    def report(self, name="native.md"):
        evidence = self.root.parent / name
        evidence.write_text("Fixture native report naming the build and the committed frames.\n")
        return evidence

    def commit_evidence(self, record, task_id, files):
        """The evidence owner's commit on top of the candidate, in their own clone, under their own name."""
        owner = self.root.parent / f"owner-{task_id}-{record['candidate'][:10]}"
        git(self.root.parent, "clone", "--quiet", "--", str(Path(record["directory"]) / "repo"), str(owner))
        git(owner, "switch", "--quiet", "--detach", record["candidate"])
        git(owner, "config", "user.name", "Evidence Owner")
        for path, content in files.items():
            (owner / path).parent.mkdir(parents=True, exist_ok=True)
            (owner / path).write_bytes(content)
        git(owner, "add", "-A")
        git(owner, "commit", "--quiet", "-m", f"docs(evidence): record {task_id}")
        return owner, head(owner)

    def parent(self, repo, commit):
        return git(repo, "rev-list", "--parents", "-n", "1", commit).split()[1:]

    def test_a_clean_rebase_keeps_the_attempt_and_reuses_the_verification_of_an_unchanged_patch(self):
        directory = self.create([fixtures.evidence_task(), fixtures.task("two")])
        gated = []

        def gates(repo, profiles, gate_directory):
            gated.append((Path(gate_directory).parent.name, head(repo)))
            return {"passed": True, "checks": []}

        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter, gates)
        record = state["tasks"]["one"]
        [history] = record["rebased_from"]
        old = history["candidate"]
        # No implementer and no verifier ran again for `one`, whose patch is unchanged; the
        # security review must see it on its new base, so it alone ran again.
        self.assertEqual(adapter.calls, [("implementer", "one"), ("verifier", "one"), ("security-reviewer", "one"),
                                         ("implementer", "two"), ("verifier", "two"), ("security-reviewer", "one")])
        self.assertEqual((adapter.sessions[-1][0], adapter.repos[-1][1]), ("security-reviewer", Path(record["directory"]) / "repo"))
        self.assertEqual((record["status"], record["attempts"]), ("awaiting_evidence", 1))
        self.assertEqual(record["reason"], f"rebased onto {state['accepted_head']}; required external evidence: native")
        self.assertEqual((record["base"], self.parent(directory / "accepted", state["accepted_head"])), (state["accepted_head"], [history["base"]]))
        repo = Path(record["directory"]) / "repo"
        self.assertEqual(self.parent(repo, record["candidate"]), [state["accepted_head"]])
        self.assertEqual(git(repo, "log", "-1", "--format=%s%n%an", record["candidate"]).splitlines(),
                         git(repo, "log", "-1", "--format=%s%n%an", old).splitlines())
        # The gates ran again, on the replayed candidate in its own clone.
        self.assertEqual(gated[-1], (Path(record["directory"]).name, record["candidate"]))
        self.assertTrue(Path(record["directory"]).name.startswith("rebase-"))
        self.assertEqual(Path(record["directory"]).parent, directory / "attempts/one/1")
        self.assertTrue((Path(record["directory"]) / "candidate.patch").is_file())
        self.assertEqual((history["patch"], history["reused"], history["onto"]), ("unchanged", ["pre_review"], state["accepted_head"]))
        # The reused verification names the candidate its reviewer saw and binds the new inputs.
        self.assertEqual(record["pre_review_reviewed"]["candidate"], old)
        self.assertEqual(record["pre_review_inputs"]["candidate"], record["candidate"])
        self.assertEqual(read_json(Path(record["pre_review"]))["candidate"], old)
        # The security verdict is the new one, on the rebased candidate and its base.
        self.assertNotIn("security_review_reviewed", record)
        security = read_json(Path(record["security_review"]))
        self.assertEqual((security["base"], security["candidate"]), (state["accepted_head"], record["candidate"]))
        self.assertTrue(record["security_review"].startswith(record["directory"]))
        # The old attempt clone never moved.
        old_repo = Path(history["directory"]) / "repo"
        self.assertEqual((head(old_repo), clean(old_repo)), (old, True))
        self.assertIn("rebasing one attempt 1", self.output)
        with self.assertRaisesRegex(LoopError, "pending candidate"):
            attest(directory, "one", old, "native", self.report(), "Checked the superseded build.")
        attest(directory, "one", record["candidate"], "native", self.report(), "Checked the rebased build.")
        final = fixtures.FakeCodex(script=True)
        accepted = self.execute(directory, final)
        self.assertEqual(final.calls, [("verifier", "one")])
        self.assertEqual((accepted["tasks"]["one"]["status"], accepted["accepted_head"]), ("accepted", record["candidate"]))
        self.assertIn(f"That verdict graded {old}", final.context_log[0][1])

    def test_a_changed_patch_reruns_both_reviews_before_parking(self):
        directory = self.create(shared_tasks())
        # Both add the same shared file, so `one`'s replay holds less of a patch than its review saw.
        adapter = SharedFileCodex({"one": "shared\n", "two": "shared\n"})
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        self.assertEqual(adapter.calls[-2:], [("verifier", "one"), ("security-reviewer", "one")])
        self.assertEqual(adapter.calls.count(("implementer", "one")), 1)
        self.assertEqual((record["status"], record["attempts"]), ("awaiting_evidence", 1))
        self.assertEqual((record["rebased_from"][0]["patch"], record["rebased_from"][0]["reused"]), ("changed", []))
        self.assertNotIn("docs/shared.md", record["security_paths"])
        for key in ("pre_review", "security_review"):
            self.assertNotIn(key + "_reviewed", record)
            self.assertTrue(record[key].startswith(record["directory"]), record[key])
            self.assertEqual(read_json(Path(record[key]))["candidate"], record["candidate"])

    def test_a_conflicting_rebase_leaves_the_clones_clean_and_spends_a_normal_attempt(self):
        directory = self.create(shared_tasks())
        adapter = SharedFileCodex({"one": "from one\n", "two": "from two\n"})
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        stale = [line for line in self.output.splitlines() if " one: stale " in line]
        self.assertEqual(len(stale), 1)
        self.assertIn("it conflicts with the new base in docs/shared.md; a new attempt rebuilds the task", stale[0])
        self.assertEqual(adapter.calls.count(("implementer", "one")), 2)
        self.assertEqual((record["status"], record["attempts"], record["base"]), ("awaiting_evidence", 2, state["accepted_head"]))
        self.assertNotIn("rebased_from", record)
        first = directory / "attempts/one/1"
        self.assertTrue(clean(first / "repo"))
        [scratch] = first.glob("rebase-*/repo")
        self.assertTrue(clean(scratch))
        self.assertEqual(head(scratch), state["tasks"]["two"]["candidate"])
        self.assertFalse((scratch / ".git/CHERRY_PICK_HEAD").exists())

    def test_a_red_gate_after_a_clean_rebase_falls_back_to_a_normal_attempt(self):
        directory = self.create([fixtures.evidence_task(), fixtures.task("two")])
        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter, fixtures.red_on_rebase)
        record = state["tasks"]["one"]
        self.assertRegex(self.output, r"one: stale \(rebase onto [0-9a-f]{12} failed: its gates failed on the new base; inspect .*rebase-")
        self.assertEqual(adapter.calls.count(("implementer", "one")), 2)
        self.assertEqual((record["status"], record["attempts"], record["base"]), ("awaiting_evidence", 2, state["accepted_head"]))

    def rebuilt_after(self, adapter, gate=fixtures.green_gates, expected=""):
        """Run `one` (evidence) and `two`; `one`'s rebase must fail with `expected` and a new attempt rebuild it."""
        directory = self.create([fixtures.evidence_task(), fixtures.task("two")])
        state = self.execute(directory, adapter, gate)
        record = state["tasks"]["one"]
        self.assertRegex(self.output, r"one: stale \(rebase onto [0-9a-f]{12} failed: " + expected)
        self.assertEqual(adapter.calls.count(("implementer", "one")), 2)
        self.assertEqual((record["status"], record["attempts"], record["base"]), ("awaiting_evidence", 2, state["accepted_head"]))
        return directory

    def test_a_gate_limit_during_a_rebase_falls_back_to_a_normal_attempt(self):
        def limited(repo, profiles, gate_directory):
            if Path(gate_directory).parent.name.startswith("rebase-"):
                raise EnvironmentBlocked("workspace-tests gate interrupted: its log reached the size limit")
            return {"passed": True, "checks": []}

        # Not the run's budget: the candidate is not left parked on its old base for the loop to wait on.
        self.rebuilt_after(fixtures.FakeCodex(script=True), limited, "workspace-tests gate interrupted")

    def test_an_old_candidate_that_fails_its_own_checks_falls_back_to_a_normal_attempt(self):
        def gates(repo, profiles, gate_directory):
            if Path(repo).parent.parent.name == "two":
                # While `two` is gated, something writes into `one`'s parked clone.
                (Path(repo).parents[2] / "one/1/repo/stray.txt").write_text("not the candidate\n")
            return {"passed": True, "checks": []}

        self.rebuilt_after(fixtures.FakeCodex(script=True), gates, r"candidate source or HEAD changed")

    def test_a_failure_after_adoption_falls_back_to_a_normal_attempt(self):
        def tamper(repo):
            if Path(repo).parent.name.startswith("rebase-"):
                (Path(repo) / "stray.txt").write_text("written by a reviewer\n")

        self.rebuilt_after(fixtures.FakeCodex(script=True, mutate=tamper), expected="security reviewer changed candidate source")

    def test_a_stop_during_a_rebase_leaves_the_old_record_and_no_owner_waits_for_it(self):
        directory = self.create([fixtures.evidence_task(), fixtures.task("two")])

        def stopped(repo, profiles, gate_directory):
            if Path(gate_directory).parent.name.startswith("rebase-"):
                (directory / "STOP").touch()
                raise EnvironmentBlocked("workspace-tests gate interrupted: stop requested")
            return {"passed": True, "checks": []}

        state = self.execute(directory, fixtures.FakeCodex(script=True), stopped)
        record = state["tasks"]["one"]
        self.assertEqual((state["phase"], state["reason"]), ("paused", "stop requested"))
        self.assertEqual((record["status"], record["attempts"]), ("awaiting_evidence", 1))
        self.assertNotEqual(record["base"], state["accepted_head"])
        self.assertIn("interrupted", record["reason"])
        self.assertEqual(Runner(directory, adapter=fixtures.FakeCodex()).awaiting_owner(), [])

    def test_a_review_blocked_candidate_gets_the_status_its_rebase_needs(self):
        directory = self.create([fixtures.evidence_task(), fixtures.task("two")])
        reviews = []

        def block_first(value, context):
            reviews.append(value["task_id"])
            if reviews == ["one"]:
                value.update(verdict="blocked", findings=["The verifier could not run the workflow script."])
                value["criteria"][0]["status"] = "unverified"

        adapter = fixtures.FakeCodex(script=True, review=block_first)
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        self.assertIn("one: review_blocked", self.output)
        # Its blocked verification did not carry over, so the rebase ran both early reviews again and parked it.
        self.assertEqual(adapter.calls[-2:], [("verifier", "one"), ("security-reviewer", "one")])
        self.assertEqual((record["status"], record["attempts"]), ("awaiting_evidence", 1))
        self.assertEqual(record["reason"], f"rebased onto {state['accepted_head']}; required external evidence: native")
        self.assertEqual(record["rebased_from"][0]["reused"], [])
        self.assertEqual(Runner(directory, adapter=fixtures.FakeCodex()).awaiting_owner(), ["one"])

    def test_an_interrupted_rebase_reconciles_to_the_old_record_and_replays_again(self):
        directory = self.create([fixtures.evidence_task(), fixtures.task("two")])

        def dying(repo, profiles, gate_directory):
            if Path(gate_directory).parent.name.startswith("rebase-"):
                raise OSError("fixture controller died during the rebased candidate's gates")
            return {"passed": True, "checks": []}

        with self.assertRaisesRegex(OSError, "died"):
            self.execute(directory, fixtures.FakeCodex(script=True), dying)
        saved = read_json(directory / "state.json")
        record = saved["tasks"]["one"]
        self.assertEqual((saved["phase"], record["status"]), ("rebasing", "awaiting_evidence"))
        self.assertNotEqual(record["base"], saved["accepted_head"])
        old = record["candidate"]
        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        # No implementer: only the security review of the replayed candidate ran.
        self.assertEqual(adapter.calls, [("security-reviewer", "one")])
        self.assertEqual((record["status"], record["attempts"], record["base"]), ("awaiting_evidence", 1, state["accepted_head"]))
        self.assertEqual([entry["candidate"] for entry in record["rebased_from"]], [old])
        self.assertEqual(len(list((directory / "attempts/one/1").glob("rebase-*"))), 2)

    def create_with_log(self, tasks):
        """A run whose base already holds the validation log, as the repository's does."""
        specification = self.prepare(tasks)
        (self.root / LOG).write_text(GUIDANCE + OLDER)
        git(self.root, "add", "--", LOG)
        git(self.root, "commit", "--quiet", "-m", "docs: seed the validation log")
        self.original_head = head(self.root)
        with patch("agent_loop.runner.Codex.preflight", return_value="fixture Codex"):
            return create_run(self.root, specification, fixtures.CONTROLLER, self.options)

    def evidence_pair(self, *, log=None):
        """Task `one` parked with an evidence commit bound to it, and an independent `two` not yet built.

        With `log`, the base holds the validation log and the evidence commit sets it to `log`.
        """
        one = fixtures.evidence_task(profiles=("native", "performance"))
        one["scope"] += ["docs/evidence/one/**", LOG]
        two = fixtures.task("two")
        two["scope"].append(LOG)
        self.options.update(max_awaiting_evidence=1)
        self.directory = self.create([one, two]) if log is None else self.create_with_log([one, two])
        # At the cap, with --max-idle-minutes 0, the run pauses instead of building `two`.
        paused = self.execute(self.directory, fixtures.FakeCodex(script=True))
        self.assertEqual((paused["phase"], paused["tasks"]["two"]["attempts"]), ("paused", 0))
        record = paused["tasks"]["one"]
        files = {"docs/evidence/one/focus.png": FRAME} | ({LOG: log.encode()} if log else {})
        owner, evidence = self.commit_evidence(record, "one", files)
        # Native evidence rides on the commit; performance is still missing, so `one` stays parked.
        attest(self.directory, "one", record["candidate"], "native", self.report(), "Checked the frames.",
               evidence_commit=evidence, evidence_repo=owner)
        state = read_json(self.directory / "state.json")
        state["max_awaiting_evidence"] = 2
        atomic_json(self.directory / "state.json", state)
        return state["tasks"]["one"], evidence

    def test_a_rebase_replays_the_evidence_commit_and_asks_only_for_a_recheck(self):
        before, evidence = self.evidence_pair()
        state = self.execute(self.directory, fixtures.FakeCodex(script=True))
        self.assertEqual(state["tasks"]["two"]["status"], "accepted")
        record = state["tasks"]["one"]
        replayed = record["evidence_commit"]
        repo = Path(record["directory"]) / "repo"
        self.assertNotEqual(replayed, evidence)
        self.assertEqual(self.parent(repo, replayed), [record["candidate"]])
        self.assertEqual(git(repo, "diff", "--name-only", record["candidate"], replayed).split(), ["docs/evidence/one/focus.png"])
        # The owner keeps authorship; the controller's identity commits the replay.
        self.assertEqual(git(repo, "log", "-1", "--format=%an|%cn", replayed).strip(), "Evidence Owner|Loop Test")
        self.assertIn(replayed, git(repo, "for-each-ref", "--format=%(refname)", "refs/gitturtle/evidence/one"))
        # Every attestation named the old candidate.
        self.assertNotIn("attestations", record)
        self.assertEqual(record["recheck"], {"evidence_commit": replayed, "frames_from": evidence, "evidence_repo": str(repo)})
        self.assertEqual(record["rebased_from"][0]["evidence_commit"], evidence)
        self.assertEqual(record["reason"], (
            f"rebased onto {state['accepted_head']}; re-check the committed frames with `qa.py recheck` against {replayed} "
            f"and attest with --evidence-commit {replayed} --evidence-repo {repo}"))
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(main(["status", "--run", str(self.directory)]), 0)
        summary = json.loads(output.getvalue())["rebases"]["one"]
        self.assertEqual((summary["candidate"], summary["recheck_wanted"], summary["rebased_from"][0]["candidate"]),
                         (record["candidate"], True, before["candidate"]))
        # Its frames came from the old build, so no kind may be attested without vouching for the replayed commit.
        with self.assertRaisesRegex(LoopError, f"carries the replayed evidence commit {replayed}"):
            attest(self.directory, "one", record["candidate"], "performance", self.report("perf.md"), "Measured.")
        # The re-check attests the replayed commit without retaking a frame, and acceptance lands on it.
        for kind in ("native", "performance"):
            self.assertTrue(recheck_wanted(read_json(self.directory / "state.json")["tasks"]["one"]))
            attest(self.directory, "one", record["candidate"], kind, self.report(f"recheck-{kind}.md"), f"Re-checked {kind}.",
                   evidence_commit=replayed, evidence_repo=repo)
        self.assertFalse(recheck_wanted(read_json(self.directory / "state.json")["tasks"]["one"]))
        adapter = fixtures.FakeCodex(script=True)
        accepted = self.execute(self.directory, adapter)
        self.assertEqual(adapter.calls, [("verifier", "one")])
        self.assertEqual((accepted["tasks"]["one"]["attempts"], accepted["accepted_head"]), (1, replayed))

    def test_an_entry_beside_the_new_bases_entry_is_stacked_above_it(self):
        _, evidence = self.evidence_pair(log=GUIDANCE + entry("one") + OLDER)
        prepare = rebase.prepare

        def without_info(source, destination, *arguments, **options):
            # As a clone made from an init.templateDir that has no info/ directory.
            prepare(source, destination, *arguments, **options)
            shutil.rmtree(destination / ".git/info")

        # `two` lands a dated entry of its own at the same place, so a plain replay conflicts in the log.
        with patch("agent_loop.rebase.prepare", without_info):
            state = self.execute(self.directory, LoggingCodex(script=True, log=GUIDANCE + entry("two") + OLDER))
        record = state["tasks"]["one"]
        self.assertEqual(state["tasks"]["two"]["status"], "accepted")
        replayed = record["evidence_commit"]
        repo = Path(record["directory"]) / "repo"
        # Both entries, the newest first, and the frames.
        self.assertEqual(git(repo, "show", f"{replayed}:{LOG}"), GUIDANCE + entry("one") + entry("two") + OLDER)
        self.assertEqual(git(repo, "diff", "--name-only", record["candidate"], replayed).split(), ["docs/evidence/one/focus.png", LOG])
        self.assertEqual(git(repo, "show", f"{replayed}:docs/evidence/one/focus.png").encode("utf-8", "surrogateescape"), FRAME)
        self.assertEqual(git(repo, "log", "-1", "--format=%an|%cn|%s", replayed).strip(), "Evidence Owner|Loop Test|docs(evidence): record one")
        self.assertEqual(record["recheck"]["frames_from"], evidence)
        self.assertIn("re-check the committed frames", record["reason"])
        self.assertFalse((repo / ".git/info/attributes").exists())

    def test_an_evidence_commit_that_edits_what_the_new_base_edited_is_dropped(self):
        _, evidence = self.evidence_pair(log=GUIDANCE + OLDER.replace("Old entry.", "Old entry, corrected by one."))
        adapter = LoggingCodex(script=True, log=GUIDANCE + OLDER.replace("Old entry.", "Old entry, corrected by two."))
        state = self.execute(self.directory, adapter)
        record = state["tasks"]["one"]
        self.assertEqual(state["tasks"]["two"]["status"], "accepted")
        # The candidate itself replayed cleanly and keeps its attempt; only its evidence commit is gone.
        self.assertEqual(adapter.calls.count(("implementer", "one")), 0)
        self.assertEqual((record["status"], record["attempts"], record["base"]), ("awaiting_evidence", 1, state["accepted_head"]))
        for key in ("evidence_commit", "recheck", "attestations"):
            self.assertNotIn(key, record)
        self.assertEqual(record["rebased_from"][0]["evidence_commit"], evidence)
        self.assertEqual(record["reason"], (
            f"rebased onto {state['accepted_head']}; its evidence commit {evidence[:12]} was dropped: it conflicts with "
            f"the new base in {LOG} and its union replay was refused: its {LOG} change is not one the replay can stack: "
            "it removes or edits lines, or adds them where the new base changed the log; "
            "required external evidence: native, performance"))
        repo = Path(record["directory"]) / "repo"
        self.assertEqual((head(repo), clean(repo)), (record["candidate"], True))

    def test_no_new_attempt_starts_while_the_evidence_cap_is_reached(self):
        self.options.update(max_awaiting_evidence=1)
        directory = self.create([fixtures.task(profiles=["native"]), fixtures.task("two")])
        adapter = fixtures.FakeCodex()
        state = self.execute(directory, adapter)
        # No criterion of `one` waits for evidence, so it is parked without a pre-evidence verifier.
        self.assertEqual(adapter.calls, [("implementer", "one")])
        self.assertEqual((state["phase"], state["tasks"]["two"]["attempts"]), ("paused", 0))
        self.assertEqual(state["reason"], "--max-awaiting-evidence 1 reached; attest or finish the pending candidates, then resume")

    def test_evidence_queued_during_the_idle_wait_reopens_its_task_without_charging_time(self):
        self.options.update(max_awaiting_evidence=1, max_idle_minutes=5)
        directory = self.create([fixtures.task(profiles=["native"]), fixtures.task("two")])
        observed = []

        def operator(runner):
            if not observed:
                saved = read_json(directory / "state.json")
                observed.append((saved["phase"], saved["budget_running"]))
                candidate = saved["tasks"]["one"]["candidate"]
                self.assertEqual(attest(directory, "one", candidate, "native", self.report(), "Checked."), "queued")
                time.sleep(0.3)  # real idle time, which the budget must not be charged

        started = time.monotonic()
        state = self.run_loop(directory, fixtures.FakeCodex(), operator=operator)
        wall = time.monotonic() - started
        self.assertEqual(observed, [("waiting_for_evidence", False)])
        self.assertEqual(state["phase"], "complete")
        self.assertEqual([state["tasks"][key]["status"] for key in ("one", "two")], ["accepted", "accepted"])
        self.assertIn("waiting for evidence on one (--max-awaiting-evidence 1 reached); idling at most 5 minutes", self.output)
        self.assertIn("wait ended after 0 minutes: evidence arrived for one", self.output)
        self.assertIn("inbox attest for one: applied", self.output)
        # Ingestion waited for the poll interval; the stop flag was checked every tick.
        self.assertAlmostEqual(sum(self.sleeps), IDLE_POLL)
        self.assertLessEqual(max(self.sleeps), IDLE_TICK)
        self.assertGreaterEqual(self.runner.idle_seconds, 0.3)
        self.assertGreaterEqual(state["remaining_seconds"], 5 * 60 - wall + 0.3)

    def test_stop_ends_the_wait_and_pauses_the_run(self):
        self.options.update(max_idle_minutes=5)
        directory = self.create([fixtures.task(profiles=["native"])])
        state = self.run_loop(directory, fixtures.FakeCodex(), operator=lambda runner: (directory / "STOP").touch())
        self.assertEqual((state["phase"], state["reason"]), ("paused", "stop requested"))
        self.assertIn("waiting for evidence on one (no other eligible work)", self.output)
        self.assertIn("wait ended after 0 minutes: stop requested", self.output)
        self.assertEqual(len(self.sleeps), 1)

    def test_the_idle_limit_pauses_the_run_with_its_reason(self):
        self.options.update(max_idle_minutes=1)
        directory = self.create([fixtures.task(profiles=["native"])])
        state = self.run_loop(directory, fixtures.FakeCodex())
        self.assertEqual((state["phase"], state["tasks"]["one"]["status"]), ("paused", "awaiting_evidence"))
        self.assertEqual(state["reason"], "waited 1 minutes for evidence on one and none arrived; attest, then resume")
        self.assertIn("wait ended after 1 minutes: waited 1 minutes for evidence on one and none arrived", self.output)
        self.assertAlmostEqual(sum(self.sleeps), 60)

    def test_b_is_built_while_a_awaits_evidence_then_rebased_and_accepted_after_its_recheck(self):
        contracts = []
        for identifier in ("a", "b"):
            contract = fixtures.evidence_task(identifier)
            contract["scope"] += [f"docs/evidence/{identifier}/**", LOG]
            contracts.append(contract)
        self.options.update(max_idle_minutes=10)
        directory = self.create_with_log(contracts)
        evidence = {}
        replayed_log = []

        def operator(runner):
            saved = read_json(directory / "state.json")
            records = saved["tasks"]
            if not evidence:
                # First wait: both candidates are parked; the owner commits and attests both, each
                # evidence commit adding its dated entry at the same place in the log.
                self.assertEqual([records[key]["status"] for key in ("a", "b")], ["awaiting_evidence", "awaiting_evidence"])
                for key in ("a", "b"):
                    files = {f"docs/evidence/{key}/focus.png": FRAME, LOG: (GUIDANCE + entry(key) + OLDER).encode()}
                    owner, commit = self.commit_evidence(records[key], key, files)
                    evidence[key] = commit
                    self.assertEqual(attest(directory, key, records[key]["candidate"], "native", self.report(f"{key}.md"),
                                            f"Checked {key}.", evidence_commit=commit, evidence_repo=owner), "queued")
            elif "recheck" not in evidence and records["b"].get("recheck"):
                # Second wait: `a` landed, `b` was replayed onto it; the owner re-checks and attests the replay.
                recheck = records["b"]["recheck"]
                evidence["recheck"] = recheck["evidence_commit"]
                self.assertIn("re-check the committed frames", records["b"]["reason"])
                replayed_log.append(git(Path(recheck["evidence_repo"]), "show", f"{recheck['evidence_commit']}:{LOG}"))
                self.assertEqual(attest(directory, "b", records["b"]["candidate"], "native", self.report("b-recheck.md"),
                                        "Re-checked the committed frames.", evidence_commit=recheck["evidence_commit"],
                                        evidence_repo=Path(recheck["evidence_repo"])), "queued")

        adapter = fixtures.FakeCodex(script=True)
        state = self.run_loop(directory, adapter, operator=operator)
        a, b = state["tasks"]["a"], state["tasks"]["b"]
        self.assertEqual(state["phase"], "complete")
        self.assertEqual(adapter.calls, [
            ("implementer", "a"), ("verifier", "a"), ("security-reviewer", "a"),
            # `b` is built while `a` awaits its evidence.
            ("implementer", "b"), ("verifier", "b"), ("security-reviewer", "b"),
            ("verifier", "a"),
            # `b` was rebased without an implementer; its unchanged patch kept the early verification,
            # and the security review saw it again on its new base.
            ("security-reviewer", "b"),
            ("verifier", "b"),
        ])
        self.assertEqual((a["status"], a["attempts"], a["evidence_commit"]), ("accepted", 1, evidence["a"]))
        self.assertEqual((b["status"], b["attempts"], b["evidence_commit"]), ("accepted", 1, evidence["recheck"]))
        self.assertEqual(state["accepted_head"], evidence["recheck"])
        accepted = directory / "accepted"
        self.assertEqual(self.parent(accepted, b["evidence_commit"]), [b["candidate"]])
        self.assertEqual(self.parent(accepted, b["candidate"]), [evidence["a"]])
        self.assertEqual(self.parent(accepted, evidence["a"]), [a["candidate"]])
        self.assertEqual(b["rebased_from"][0]["evidence_commit"], evidence["b"])
        # `b`'s replayed evidence keeps both dated entries, the newest first, and its frames land with it.
        expected = GUIDANCE + entry("b") + entry("a") + OLDER
        self.assertEqual(replayed_log, [expected])
        self.assertEqual(git(accepted, "show", f"HEAD:{LOG}"), expected)
        self.assertEqual(git(accepted, "ls-tree", "--name-only", "-r", "HEAD", "docs/evidence").split(),
                         ["docs/evidence/a/focus.png", "docs/evidence/b/focus.png"])
        events = [line.split(" ", 1)[1] for line in self.output.splitlines()]
        self.assertIn("waiting for evidence on a, b (no other eligible work); idling at most 10 minutes", events)
        self.assertIn("wait ended after 0 minutes: evidence arrived for a, b", events)
        self.assertIn("rebasing b attempt 1", events)
        self.assertTrue(any(event.startswith(f"b: awaiting_evidence (rebased onto {evidence['a']}; re-check the committed frames")
                            for event in events), events)
        self.assertIn("wait ended after 0 minutes: evidence arrived for b", events)
        self.assertEqual(sum(self.sleeps), 2 * IDLE_POLL)

    def test_stacking_places_each_inserted_block_above_the_new_bases_and_refuses_edits(self):
        base = ["# Log\n", "\n", "## old\n"]
        theirs = ["# Log\n", "\n", "## b\n", "\n", "## old\n"]
        ours = ["# Log\n", "\n", "## a\n", "\n", "## old\n", "tail\n"]
        inserted, changed = [(2, 0, 3, 2)], [(2, 0, 3, 2), (3, 0, 6, 1)]
        self.assertEqual(stack_insertions(base, ours, theirs, inserted, changed),
                         ["# Log\n", "\n", "## b\n", "\n", "## a\n", "\n", "## old\n", "tail\n"])
        # Both added the whole file: the newer first.
        self.assertEqual(stack_insertions([], ["a\n"], ["b\n"], [(0, 0, 1, 1)], [(0, 0, 1, 1)]), ["b\n", "a\n"])
        # The evidence commit edits a line, or inserts inside lines the new base replaced.
        self.assertIsNone(stack_insertions(base, ours, ["# Log\n", "\n", "## old!\n"], [(3, 1, 3, 1)], changed))
        # The new base replaced lines 2 and 3; the evidence commit inserts between them.
        self.assertIsNone(stack_insertions(base, ["# Log\n", "## new\n"], ["# Log\n", "\n", "## b\n", "## old\n"],
                                           [(2, 0, 3, 1)], [(2, 2, 2, 1)]))
        # Hunks that do not rebuild the new base's log are refused rather than trusted.
        self.assertIsNone(stack_insertions(base, ours, theirs, inserted, [(2, 0, 3, 1)]))

    def test_resume_renews_the_concurrency_bounds_and_a_new_run_records_the_defaults(self):
        del self.options["max_idle_minutes"]
        directory = self.create()
        state = read_json(directory / "state.json")
        self.assertEqual((state["max_awaiting_evidence"], state["max_idle_minutes"]), (MAX_AWAITING_EVIDENCE, MAX_IDLE_MINUTES))
        with patch.object(Runner, "execute", lambda runner: runner.state), redirect_stdout(io.StringIO()):
            main(["resume", "--run", str(directory), "--max-awaiting-evidence", "5", "--max-idle-minutes", "0"])
        state = read_json(directory / "state.json")
        self.assertEqual((state["max_awaiting_evidence"], state["max_idle_minutes"]), (5, 0))


if __name__ == "__main__":
    unittest.main()
