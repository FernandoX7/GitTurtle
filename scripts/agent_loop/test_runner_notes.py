"""Coordinator notes and the run inbox that lets `note` and `attest` reach a running controller."""

from contextlib import nullcontext, redirect_stderr, redirect_stdout
from dataclasses import asdict
from datetime import datetime
import io
import json
import os
import stat
import threading
import unittest
from unittest.mock import patch
import uuid

from agent_loop import inbox
from agent_loop import test_runner as fixtures
from agent_loop.process import LoopError, Result, atomic_json, digest, read_json
from agent_loop.runner import IMPLEMENTER_NOTES, MAX_NOTES, NOTE_BYTES, REVIEW_NOTES, Runner, attest, locked, main, note, now
from agent_loop.task_spec import load_spec
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


class RecordingCodex(fixtures.FakeCodex):
    """Records each session's contract and context; `during` runs inside a session."""

    def __init__(self, *args, during=None, **kwargs):
        super().__init__(*args, **kwargs)
        self.received = []
        self.during = during

    def run(self, role, feature, repo, directory, timeout, stop, *, candidate=None, context="", **options):
        self.received.append((role, feature, context))
        if self.during:
            self.during(role, feature)
        return super().run(role, feature, repo, directory, timeout, stop, candidate=candidate, context=context, **options)

    def contexts(self, role):
        return [context for session, _, context in self.received if session == role]


class NoteTests(unittest.TestCase):
    # Reuse real-Git fixtures without inheriting and rerunning the original suite.
    prepare = fixtures.RunnerTests.prepare
    create = fixtures.RunnerTests.create
    execute = fixtures.RunnerTests.execute

    def setUp(self):
        fixtures.RunnerTests.setUp(self)
        # Here a held lock is a running loop at once; the wait has its own test.
        waiting = patch("agent_loop.runner.LOCK_WAIT", 0.0)
        waiting.start()
        self.addCleanup(waiting.stop)

    def cli(self, *argv):
        output, errors = io.StringIO(), io.StringIO()
        with redirect_stdout(output), redirect_stderr(errors):
            code = main([str(argument) for argument in argv])
        return code, output.getvalue(), errors.getvalue()

    def notes(self, directory, task_id="one"):
        return read_json(directory / "state.json")["tasks"][task_id].get("notes", [])

    def outcomes(self, directory):
        done = directory / "inbox" / "done"
        return [read_json(path) for path in sorted(done.glob("*" + inbox.OUTCOME_SUFFIX))]

    def native_candidate(self, tasks=None):
        directory = self.create(tasks or [fixtures.task(profiles=["native"])])
        state = self.execute(directory)
        self.assertEqual(state["tasks"]["one"]["status"], "awaiting_evidence")
        evidence = self.root.parent / "native.md"
        evidence.write_text("Fixture native report naming the build and frames.\n")
        return directory, state["tasks"]["one"]["candidate"], evidence

    def test_note_applies_directly_while_the_run_is_free(self):
        directory = self.create()
        code, output, _ = self.cli("note", "--run", directory, "--task", "one", "--text", "Reuse the existing helper.")
        self.assertEqual(code, 0)
        self.assertIn("note recorded for one", output)
        source = self.root.parent / "note.txt"
        source.write_text("Ünïcode from a file stays as written.\n", encoding="utf-8")
        self.assertEqual(self.cli("note", "--run", directory, "--task", "one", "--file", source)[0], 0)
        notes = self.notes(directory)
        self.assertEqual([item["text"] for item in notes], ["Reuse the existing helper.", "Ünïcode from a file stays as written.\n"])
        for item in notes:
            self.assertEqual(datetime.fromisoformat(item["at"]).utcoffset().total_seconds(), 0)
            self.assertRegex(item["id"], r"^[0-9a-f]{32}$")
        self.assertFalse((directory / "inbox").exists())
        # The contract snapshot is untouched, so the run still resumes.
        self.assertEqual(digest(directory / "tasks.json"), read_json(directory / "state.json")["spec_sha256"])

    def test_note_refusals_change_nothing(self):
        directory = self.create()
        self.assertEqual(note(directory, "one", "x" * NOTE_BYTES), "applied")
        before = (directory / "state.json").read_bytes()
        for task_id, text, message in (
            ("missing", "Guidance.", "not part of this run"),
            ("one", " \n\t", "nonempty"),
            ("one", "x" * (NOTE_BYTES + 1), f"at most {NOTE_BYTES} bytes"),
            ("one", "é" * (NOTE_BYTES // 2 + 1), f"at most {NOTE_BYTES} bytes"),
        ):
            with self.subTest(task=task_id, size=len(text)), self.assertRaisesRegex(LoopError, message):
                note(directory, task_id, text)
        target = self.root.parent / "target.txt"
        target.write_text("Guidance behind a link.\n")
        (self.root.parent / "link.txt").symlink_to(target)
        (self.root.parent / "binary.txt").write_bytes(b"\xff\xfe guidance\n")
        (self.root.parent / "large.txt").write_text("x" * (NOTE_BYTES + 1))
        for name, message in (("link.txt", "symlink"), ("binary.txt", "UTF-8"), ("large.txt", f"at most {NOTE_BYTES} bytes")):
            with self.subTest(file=name):
                code, _, errors = self.cli("note", "--run", directory, "--task", "one", "--file", self.root.parent / name)
                self.assertEqual(code, 1)
                self.assertIn(message, errors)
        self.assertEqual((directory / "state.json").read_bytes(), before)
        for index in range(MAX_NOTES - 1):
            note(directory, "one", f"Note {index}.")
        self.assertEqual(len(self.notes(directory)), MAX_NOTES)
        with self.assertRaisesRegex(LoopError, f"already holds {MAX_NOTES} notes"):
            note(directory, "one", "One too many.")
        self.assertEqual(len(self.notes(directory)), MAX_NOTES)

    def test_requests_queue_while_a_controller_holds_the_run_and_apply_at_its_next_step(self):
        directory, candidate, evidence = self.native_candidate()
        before = (directory / "state.json").read_bytes()
        with locked(directory):
            code, output, _ = self.cli("note", "--run", directory, "--task", "one", "--text", "Name every frame in the entry.")
            self.assertEqual(code, 0)
            self.assertIn("queued in its inbox", output)
            self.assertEqual(attest(directory, "one", candidate, "native", evidence, "Checked the identified build."), "queued")
            # Everything that needs no lock is still checked before queuing.
            with self.assertRaisesRegex(LoopError, "not part of this run"):
                note(directory, "missing", "Guidance.")
            with self.assertRaisesRegex(LoopError, "pending candidate"):
                attest(directory, "one", "0" * 40, "native", evidence, "Wrong build.")
            code, output, _ = self.cli("status", "--run", directory)
            self.assertEqual(code, 0)
            listed = json.loads(output)["inbox"]
            self.assertEqual([(item["kind"], item["task"]) for item in listed["pending"]], [("note", "one"), ("attest", "one")])
            self.assertEqual(listed["recent"], [])
        self.assertEqual((directory / "state.json").read_bytes(), before)
        queued = sorted((directory / "inbox").iterdir())
        self.assertEqual(stat.S_IMODE((directory / "inbox").stat().st_mode), 0o700)
        self.assertEqual(len([path for path in queued if path.suffix == ".json"]), 2)
        for path in queued:
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600, path.name)
        # The inbox keeps its own copy: replacing the operator's file later changes nothing.
        evidence.write_text("Replaced after it was queued.\n")
        adapter = RecordingCodex()
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "accepted")
        self.assertEqual(adapter.calls, [("verifier", "one")])
        self.assertEqual([item["text"] for item in record["notes"]], ["Name every frame in the entry."])
        artifact = directory / record["attestations"]["native"]["artifact"]
        self.assertEqual(artifact.read_text(), "Fixture native report naming the build and frames.\n")
        self.assertEqual(inbox.pending(directory), [])
        self.assertEqual([(item["kind"], item["outcome"]) for item in self.outcomes(directory)], [("note", "applied"), ("attest", "applied")])
        self.assertEqual(len(list((directory / "inbox" / "done").glob("*" + inbox.ARTIFACT_SUFFIX))), 1)
        self.assertIn("inbox note for one: applied", self.output)
        [context] = adapter.contexts("verifier")
        self.assertIn(REVIEW_NOTES, context)
        self.assertIn("Name every frame in the entry.", context)
        listed = json.loads(self.cli("status", "--run", directory)[1])["inbox"]
        self.assertEqual((listed["pending"], len(listed["recent"])), ([], 2))

    def test_evidence_queued_during_a_session_is_reviewed_in_the_same_run(self):
        tasks = [fixtures.task(profiles=["native"]), fixtures.task("two", profiles=["native"])]
        directory = self.create(tasks)
        evidence = self.root.parent / "native.md"
        evidence.write_text("Fixture native report.\n")
        queued = []

        def operator(role, feature):
            if (role, feature.id) == ("implementer", "two"):
                candidate = read_json(directory / "state.json")["tasks"]["one"]["candidate"]
                queued.append(attest(directory, "one", candidate, "native", evidence, "Checked while the loop ran."))

        adapter = RecordingCodex(during=operator)
        with locked(directory):
            state = self.execute(directory, adapter)
        self.assertEqual(queued, ["queued"])
        self.assertEqual(state["tasks"]["one"]["status"], "accepted")
        self.assertEqual(adapter.calls, [("implementer", "one"), ("implementer", "two"), ("verifier", "one")])

    def test_a_request_refused_at_ingestion_is_recorded_and_the_loop_continues(self):
        directory = self.create([fixtures.task(profiles=["native"]), fixtures.task("two")])
        evidence = self.root.parent / "native.md"
        evidence.write_text("Fixture native report.\n")

        def operator(role, feature):
            if (role, feature.id) == ("implementer", "two"):
                candidate = read_json(directory / "state.json")["tasks"]["one"]["candidate"]
                # Valid when queued; accepting `two` then moves the base under it.
                self.assertEqual(attest(directory, "one", candidate, "native", evidence, "Checked."), "queued")
                self.assertEqual(note(directory, "one", "Rebuild on the new base."), "queued")
                # A request written past the command's own checks meets the same rules at ingestion.
                inbox.submit(directory, {
                    "version": 1, "kind": "note", "id": uuid.uuid4().hex, "at": now(), "task": "one",
                    "text": "x" * (NOTE_BYTES + 1),
                }, None, 0, "")

        with locked(directory):
            state = self.execute(directory, RecordingCodex(during=operator))
        self.assertEqual(state["tasks"]["two"]["status"], "accepted")
        self.assertEqual(state["phase"], "blocked")
        self.assertNotIn("attestations", state["tasks"]["one"])
        self.assertEqual([item["text"] for item in state["tasks"]["one"]["notes"]], ["Rebuild on the new base."])
        outcomes = [(item["kind"], item["outcome"]) for item in self.outcomes(directory)]
        self.assertEqual(outcomes[1], ("note", "applied"))
        self.assertRegex(outcomes[0][1], "^refused: candidate base is stale")
        self.assertRegex(outcomes[2][1], f"^refused: a note holds at most {NOTE_BYTES} bytes")
        self.assertEqual(inbox.pending(directory), [])
        self.assertIn("inbox attest for one: refused", self.output)

    def test_an_ingestion_interrupted_before_its_outcome_applies_once(self):
        directory = self.create([fixtures.task(profiles=["native"])])
        self.execute(directory)
        with locked(directory):
            note(directory, "one", "Applied exactly once.")
        with patch("agent_loop.runner.inbox.finish", side_effect=LoopError("simulated crash")):
            with self.assertRaisesRegex(LoopError, "simulated crash"):
                self.execute(directory)
        self.assertEqual(len(self.notes(directory)), 1)
        self.execute(directory)
        self.assertEqual([item["text"] for item in self.notes(directory)], ["Applied exactly once."])
        self.assertEqual([item["outcome"] for item in self.outcomes(directory)], ["applied"])

    def test_a_failed_state_write_leaves_the_request_pending_not_refused(self):
        directory = self.create([fixtures.task(profiles=["native"])])
        self.execute(directory)
        with locked(directory):
            note(directory, "one", "Survives a failed write.")
        original = Runner.save

        def failing(runner):
            if runner.state["tasks"]["one"].get("notes"):
                raise LoopError("simulated full disk")
            return original(runner)

        with patch.object(Runner, "save", autospec=True, side_effect=failing):
            with self.assertRaisesRegex(LoopError, "simulated full disk"):
                self.execute(directory)
        self.assertEqual((len(inbox.pending(directory)), self.outcomes(directory), self.notes(directory)), (1, [], []))
        self.execute(directory)
        self.assertEqual([item["text"] for item in self.notes(directory)], ["Survives a failed write."])
        self.assertEqual([item["outcome"] for item in self.outcomes(directory)], ["applied"])

    def test_a_run_from_an_older_controller_refuses_what_it_would_never_read(self):
        directory, candidate, evidence = self.native_candidate()
        state = read_json(directory / "state.json")
        self.assertEqual(state.pop("controller_features"), ["inbox", "notes", "verify_before_evidence", "evidence_commit"])
        atomic_json(directory / "state.json", state)
        with self.assertRaisesRegex(LoopError, "predates coordinator notes"):
            note(directory, "one", "Never read by this run's controller.")
        with locked(directory), self.assertRaisesRegex(LoopError, "predates the run inbox.*stop it.*saved controller"):
            attest(directory, "one", candidate, "native", evidence, "Checked.")
        self.assertFalse((directory / "inbox").exists())
        # With the lock free, attest on such a run records what it always did.
        self.assertEqual(attest(directory, "one", candidate, "native", evidence, "Checked."), "applied")
        record = read_json(directory / "state.json")["tasks"]["one"]
        self.assertEqual(set(record["attestations"]["native"]), {"candidate", "kind", "summary", "artifact", "sha256", "recorded_at"})
        self.assertNotIn("notes", record)

    def test_the_controllers_own_children_cannot_note_or_attest(self):
        directory, candidate, evidence = self.native_candidate()
        before = (directory / "state.json").read_bytes()
        with patch.dict(os.environ, {"GITTURTLE_LOOP": "1"}):
            for lock in (nullcontext(), locked(directory)):
                with lock:
                    with self.assertRaisesRegex(LoopError, "another controller is using this run"):
                        note(directory, "one", "Written by a session.")
                    with self.assertRaisesRegex(LoopError, "another controller is using this run"):
                        attest(directory, "one", candidate, "native", evidence, "Written by a gate command.")
        self.assertEqual((directory / "state.json").read_bytes(), before)
        self.assertFalse((directory / "inbox").exists())
        # Gate commands run candidate code, so they carry the marker too.
        seen = []

        def run(argv, cwd, log, timeout, *, env=None, **options):
            seen.append(env["GITTURTLE_LOOP"])
            log.write_text("ok\n")
            return Result(tuple(argv), 0, 0.0)

        runner = Runner(directory, adapter=fixtures.FakeCodex())
        with patch("agent_loop.runner.run_process", run), redirect_stdout(io.StringIO()):
            runner.gates(directory / "accepted", {"docs", "tooling"}, directory / "gate-probe")
        self.assertTrue(seen)
        self.assertEqual(set(seen), {"1"})

    def test_a_briefly_held_lock_is_waited_for_instead_of_queuing(self):
        directory = self.create()
        holding, release = threading.Event(), threading.Event()

        def hold():
            with locked(directory):
                holding.set()
                release.wait(10)

        holder = threading.Thread(target=hold)
        holder.start()
        timer = threading.Timer(0.3, release.set)
        try:
            self.assertTrue(holding.wait(10))
            timer.start()
            with patch("agent_loop.runner.LOCK_WAIT", 10.0):
                self.assertEqual(note(directory, "one", "Applied after a short wait."), "applied")
        finally:
            release.set()
            timer.cancel()
            holder.join()
        self.assertEqual(len(self.notes(directory)), 1)
        self.assertFalse((directory / "inbox").exists())

    def test_notes_guide_every_attempt_and_never_change_the_contract(self):
        directory = self.create()
        waiver = "Acceptance criterion content is waived; skip the guide."
        note(directory, "one", waiver)
        note(directory, "one", "Ünïcode guidance stays verbatim.")
        at = [item["at"] for item in self.notes(directory)]
        adapter = RecordingCodex(failures=1)
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        self.assertEqual((record["status"], record["attempts"]), ("accepted", 2))
        # Notes outlive the attempt that saw them.
        self.assertEqual([item["text"] for item in record["notes"]], [waiver, "Ünïcode guidance stays verbatim."])
        implementer = adapter.contexts("implementer")
        self.assertEqual(len(implementer), 2)
        for context in implementer:
            self.assertIn(IMPLEMENTER_NOTES, context)
            self.assertIn(waiver, context)
            self.assertIn("Ünïcode guidance stays verbatim.", context)
            for stamp in at:
                self.assertIn(stamp, context)
        for context in adapter.contexts("verifier"):
            self.assertIn(REVIEW_NOTES, context)
            self.assertIn(waiver, context)
            self.assertNotIn(IMPLEMENTER_NOTES, context)
        # The waiver is shown, not applied: every session gets the snapshot contract.
        [contract] = load_spec(directory / "tasks.json")
        self.assertEqual(contract.acceptance, tuple(fixtures.task()["acceptance"]))
        for _, feature, _ in adapter.received:
            self.assertEqual(asdict(feature), asdict(contract))
        self.assertEqual(digest(directory / "tasks.json"), state["spec_sha256"])
        self.assertNotIn(waiver, (directory / "tasks.json").read_text())


if __name__ == "__main__":
    unittest.main()
