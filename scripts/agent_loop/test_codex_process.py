"""Exercise the CLI contract with fake executables and disposable processes."""

from __future__ import annotations

from copy import deepcopy
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
from unittest.mock import patch

from agent_loop.codex import Codex, validate_review
from agent_loop.process import LoopError, run_process
from agent_loop.task_spec import Task
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


def example_task() -> Task:
    return Task(
        id="inspect-history", title="Inspect history", description="Show captured history.",
        depends_on=(), scope=("crates/app/**",),
        acceptance=(
            {"id": "selection", "description": "Selection retains the captured commit."},
            {"id": "missing-object", "description": "Missing local objects stay explicit."},
        ),
        profiles=("rust",), commit="feat: inspect captured history",
    )


def passing_review() -> dict:
    return {
        "task_id": "inspect-history", "candidate": "a" * 40, "verdict": "pass",
        "criteria": [
            {"id": "selection", "status": "pass", "evidence": "selection.log: captured OID retained"},
            {"id": "missing-object", "status": "pass", "evidence": "missing.log: explicit error"},
        ],
        "findings": [],
    }


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.task = example_task()
        self.candidate = "a" * 40

    def test_complete_review_accepts_only_its_candidate_and_task(self):
        self.assertEqual(validate_review(passing_review(), self.task, self.candidate), "pass")
        for field, value in (("candidate", "b" * 40), ("task_id", "another-task")):
            with self.subTest(field=field):
                review = passing_review()
                review[field] = value
                with self.assertRaisesRegex(LoopError, "different task or candidate"):
                    validate_review(review, self.task, self.candidate)

    def test_criteria_must_cover_contract_exactly_once(self):
        for change in ("duplicate", "missing", "unknown"):
            with self.subTest(change=change):
                review = passing_review()
                if change == "duplicate":
                    review["criteria"].append(deepcopy(review["criteria"][0]))
                elif change == "missing":
                    review["criteria"].pop()
                else:
                    review["criteria"][0]["id"] = "invented-check"
                with self.assertRaises(LoopError):
                    validate_review(review, self.task, self.candidate)

    def test_failed_and_blocked_reviews_preserve_their_outcomes(self):
        for verdict, status in (("fail", "fail"), ("blocked", "unverified")):
            with self.subTest(verdict=verdict):
                review = passing_review()
                review["verdict"] = verdict
                review["criteria"][0].update(status=status, evidence="Required native evidence is unavailable.")
                self.assertEqual(validate_review(review, self.task, self.candidate), verdict)
                review["verdict"] = "pass"
                with self.assertRaisesRegex(LoopError, "incomplete or failing"):
                    validate_review(review, self.task, self.candidate)

    def test_blank_evidence_and_wrong_field_types_are_controlled_failures(self):
        mutations = [
            ("blank evidence", lambda value: value["criteria"][0].update(evidence="  ")),
            ("verdict array", lambda value: value.update(verdict=[])),
            ("status object", lambda value: value["criteria"][0].update(status={})),
            ("criteria object", lambda value: value.update(criteria={})),
            ("findings string", lambda value: value.update(findings="looks good")),
            ("unknown field", lambda value: value.update(approved=True)),
            ("false pass", lambda value: value.update(findings=["A concrete blocking defect remains."])),
        ]
        for label, mutate in mutations:
            with self.subTest(label=label):
                review = passing_review()
                mutate(review)
                with self.assertRaises(LoopError):
                    validate_review(review, self.task, self.candidate)


@unittest.skipUnless(os.name == "posix", "The controller supports macOS and Linux processes")
class CodexProcessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "checkout"
        self.repo.mkdir()
        self.controller = self.root / "controller"
        roles = self.controller / ".codex/agents"
        roles.mkdir(parents=True)
        for role in ("implementer", "verifier", "security-reviewer"):
            (roles / f"{role}.toml").write_text(
                f'name = "{role}"\ndescription = "Fixture role"\n'
                'developer_instructions = "Inspect the fixture contract."\n', encoding="utf-8"
            )
        self.task = example_task()
        self.adapter = Codex(self.controller, "selected-model", "medium")
        self.invocation = self.root / "invocation.json"

    def fake_codex(self, *, events=None, response=None, write_response=True, exitcode=0,
                   help_text=None):
        if events is None:
            events = [{"type": "turn.completed", "usage": {"output_tokens": 37}}]
        if response is None:
            response = {"task_id": self.task.id, "status": "ready", "summary": "Fixture change is ready."}
        if help_text is None:
            help_text = "--json --output-schema --ignore-user-config --strict-config --sandbox --output-last-message"
        config = {
            "events": events, "response": response, "write_response": write_response,
            "exitcode": exitcode, "help_text": help_text,
        }
        executable = self.root / "fake-codex"
        executable.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\nfrom pathlib import Path\n"
            f"config = {config!r}\n"
            "if sys.argv[1:] == ['--version']:\n"
            "    print('codex fixture 1.0'); raise SystemExit(0)\n"
            "if sys.argv[1:] == ['exec', '--help']:\n"
            "    print(config['help_text']); raise SystemExit(0)\n"
            "args = sys.argv[1:]\n"
            "prompt = sys.stdin.read()\n"
            "capture = {'args': args, 'prompt': prompt, 'cwd': os.getcwd(),\n"
            "           'environment': {k: os.environ.get(k) for k in\n"
            "           ['CODEX_THREAD_ID', 'CODEX_TASK_ID', 'CODEX_INTERNAL_ORIGINATOR_OVERRIDE']}}\n"
            f"Path({str(self.invocation)!r}).write_text(json.dumps(capture))\n"
            "assert args[0] == 'exec' and args[-1] == '-'\n"
            "assert '--output-last-message' in args\n"
            "output = Path(args[args.index('--output-last-message') + 1])\n"
            "if config['write_response']:\n"
            "    output.write_text(json.dumps(config['response']))\n"
            "for event in config['events']:\n"
            "    print(json.dumps(event), flush=True)\n"
            "raise SystemExit(config['exitcode'])\n", encoding="utf-8"
        )
        executable.chmod(0o755)
        self.adapter.executable = str(executable)
        return executable

    def session(self, role="implementer", **options):
        return self.adapter.run(
            role, self.task, self.repo, self.root / "attempt", 5, lambda: False, **options
        )

    def test_cli_uses_selected_settings_contract_stdin_and_controlled_configuration(self):
        self.fake_codex()
        environment = {key: "parent-session" for key in (
            "CODEX_THREAD_ID", "CODEX_TASK_ID", "CODEX_INTERNAL_ORIGINATOR_OVERRIDE"
        )}
        with patch.dict(os.environ, environment):
            value = self.session(context="Gate evidence: fixture-check.log")
        self.assertEqual(value["status"], "ready")
        self.assertEqual(self.adapter.output_tokens, 37)
        capture = json.loads(self.invocation.read_text())
        args = capture["args"]
        self.assertEqual(args[args.index("--model") + 1], "selected-model")
        self.assertEqual(args[args.index("--sandbox") + 1], "workspace-write")
        self.assertIn("--strict-config", args)
        self.assertIn("--ignore-user-config", args)
        for unsafe in ("--yolo", "--dangerously-bypass-approvals-and-sandbox", "--full-auto"):
            self.assertNotIn(unsafe, args)
        overrides = {}
        for index, argument in enumerate(args):
            if argument == "-c":
                overrides.update(tomllib.loads(args[index + 1]))
        self.assertEqual(overrides["model_reasoning_effort"], "medium")
        self.assertEqual(overrides["approval_policy"], "never")
        self.assertEqual(overrides["developer_instructions"], "Inspect the fixture contract.")
        self.assertEqual(capture["environment"], {key: None for key in environment})
        self.assertEqual(Path(capture["cwd"]).resolve(), self.repo.resolve())
        self.assertIn('"id": "inspect-history"', capture["prompt"])
        self.assertIn(self.task.acceptance[1]["description"], capture["prompt"])
        self.assertIn("fixture-check.log", capture["prompt"])
        self.assertEqual(capture["prompt"], (self.root / "attempt/implementer.prompt.txt").read_text())
        self.assertTrue((self.root / "attempt/implementer.process.json").is_file())

    def test_verifier_runs_read_only_for_the_explicit_candidate(self):
        self.fake_codex(response=passing_review())
        result = self.session("verifier", candidate="a" * 40)
        capture = json.loads(self.invocation.read_text())
        args = capture["args"]
        self.assertEqual(args[args.index("--sandbox") + 1], "read-only")
        self.assertIn("Candidate: " + "a" * 40, capture["prompt"])
        self.assertEqual(validate_review(result, self.task, "a" * 40), "pass")

    def test_security_reviewer_uses_read_only_schema_and_explicit_base(self):
        from agent_loop.test_security_review import passing_security
        response = passing_security(self.task.id, "b" * 40, "a" * 40)
        self.fake_codex(response=response)
        result = self.session("security-reviewer", candidate="a" * 40, base="b" * 40)
        self.assertEqual(result, response)
        capture = json.loads(self.invocation.read_text())
        args = capture["args"]
        self.assertEqual(args[args.index("--sandbox") + 1], "read-only")
        self.assertIn("Base: " + "b" * 40, capture["prompt"])
        schema = json.loads(Path(args[args.index("--output-schema") + 1]).read_text())
        self.assertIn("coverage", schema["required"])
        self.assertEqual(self.adapter.output_tokens, 37)

    def test_security_reviewer_missing_identity_never_launches(self):
        self.fake_codex()
        for options in ({}, {"candidate": "a" * 40}, {"base": "b" * 40}):
            with self.subTest(options=options), self.assertRaisesRegex(LoopError, "explicit"):
                self.session("security-reviewer", **options)
        self.assertFalse(self.invocation.exists())

    def test_verifier_without_a_candidate_never_launches(self):
        self.fake_codex(response=passing_review())
        for candidate in (None, ""):
            with self.subTest(candidate=candidate):
                with self.assertRaisesRegex(LoopError, "explicit candidate"):
                    self.session("verifier", candidate=candidate)
        self.assertFalse(self.invocation.exists())

    def test_failed_terminal_event_rejects_valid_output_despite_exit_zero(self):
        for event_type in ("turn.failed", "error"):
            with self.subTest(event_type=event_type):
                self.fake_codex(events=[
                    {"type": "turn.completed", "usage": {"output_tokens": 5}},
                    {"type": event_type, "message": "fixture session failure"},
                ])
                directory = self.root / event_type
                with self.assertRaisesRegex(LoopError, "session incomplete"):
                    self.adapter.run("implementer", self.task, self.repo, directory, 5, lambda: False)
                process = json.loads((directory / "implementer.process.json").read_text())
                self.assertEqual(process["returncode"], 0)
                self.assertTrue((directory / "implementer.response.json").is_file())

    def test_missing_terminal_event_or_output_cannot_pass(self):
        for mode in ("no completion", "no output", "nonzero exit"):
            with self.subTest(mode=mode):
                self.fake_codex(
                    events=[] if mode == "no completion" else [{"type": "turn.completed"}],
                    write_response=mode != "no output", exitcode=7 if mode == "nonzero exit" else 0,
                )
                with self.assertRaises(LoopError):
                    self.adapter.run("implementer", self.task, self.repo, self.root / mode, 5, lambda: False)

    def test_blocked_implementation_and_malformed_status(self):
        self.fake_codex(response={"task_id": self.task.id, "status": "blocked", "summary": "Native fixture unavailable."})
        self.assertEqual(self.session()["status"], "blocked")
        self.fake_codex(response={"task_id": self.task.id, "status": [], "summary": "Invalid enum."})
        with self.assertRaises(LoopError):
            self.adapter.run("implementer", self.task, self.repo, self.root / "malformed", 5, lambda: False)

    def test_prior_output_is_preserved_without_launching_another_session(self):
        self.fake_codex()
        directory = self.root / "attempt"
        directory.mkdir()
        output = directory / "implementer.response.json"
        original = b'{"task_id":"inspect-history","status":"ready","summary":"old result"}'
        output.write_bytes(original)
        with self.assertRaisesRegex(LoopError, "output already exists"):
            self.session()
        self.assertEqual(output.read_bytes(), original)
        self.assertFalse(self.invocation.exists())

    def test_dangling_prior_output_is_refused_before_child_can_follow_it(self):
        self.fake_codex()
        directory = self.root / "attempt"
        directory.mkdir()
        destination = self.root / "outside-response.json"
        (directory / "implementer.response.json").symlink_to(destination)
        with self.assertRaises(LoopError):
            self.session()
        self.assertFalse(destination.exists())
        self.assertFalse(self.invocation.exists())

    def test_preflight_checks_capabilities_without_starting_a_session(self):
        self.fake_codex()
        self.assertEqual(self.adapter.preflight(), "codex fixture 1.0")
        self.assertFalse(self.invocation.exists())
        self.fake_codex(help_text="--json --output-schema --ignore-user-config --sandbox")
        with self.assertRaisesRegex(LoopError, "required flag --strict-config"):
            self.adapter.preflight()
        self.assertFalse(self.invocation.exists())

    def test_stdin_is_closed_and_log_cap_applies_even_to_a_fast_exit(self):
        log = self.root / "output.log"
        result = run_process(
            [sys.executable, "-c", "import sys; print(sys.stdin.read(), end='')"],
            self.repo, log, 5, stdin="prompt reached EOF",
        )
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(result.stopped)
        self.assertEqual(log.read_text(), "prompt reached EOF")
        result = run_process(
            [sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'x' * 8192)"],
            self.repo, self.root / "bounded.log", 5, max_log_bytes=256,
        )
        self.assertEqual(result.stopped, "log limit exceeded")
        self.assertEqual((self.root / "bounded.log").read_bytes(), b"x" * 256)

    def descendant_script(self, *, leader_exits=False):
        """Return a parent whose child ignores TERM and records its ready state."""
        ready = self.root / "descendant-ready"
        pids = self.root / "pids.json"
        child_code = (
            "import os, signal, time; from pathlib import Path; "
            "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
            f"Path({str(ready)!r}).write_text(str(os.getpid())); time.sleep(60)"
        )
        script = self.root / "parent.py"
        script.write_text(
            "import json, os, subprocess, sys, time\nfrom pathlib import Path\n"
            f"child = subprocess.Popen([sys.executable, '-c', {child_code!r}])\n"
            f"Path({str(pids)!r}).write_text(json.dumps([os.getpid(), child.pid]))\n"
            f"while not Path({str(ready)!r}).exists(): time.sleep(0.01)\n"
            "print('child is ready', flush=True)\n"
            + ("" if leader_exits else "time.sleep(60)\n"), encoding="utf-8"
        )

        def cleanup_group():
            if pids.exists():
                group, _ = json.loads(pids.read_text())
                try:
                    os.killpg(group, signal.SIGKILL)
                except ProcessLookupError:
                    pass

        self.addCleanup(cleanup_group)
        return script, ready, pids

    def assert_process_inactive(self, pid):
        # Orphan zombies can await init's reaper on Linux; they cannot execute.
        # Only the direct child belongs to our waitpid ownership.
        deadline = time.monotonic() + 3
        while True:
            result = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)],
                                    capture_output=True, text=True, timeout=2, check=False)
            state = result.stdout.strip()
            if not state or state.startswith("Z"):
                return
            if time.monotonic() >= deadline:
                self.fail(f"fixture descendant {pid} is still executing: {state}")
            time.sleep(0.02)

    def test_stop_terminates_term_resistant_descendants_and_reaps_leader(self):
        script, ready, pids = self.descendant_script()
        result = run_process([sys.executable, str(script)], self.repo,
                             self.root / "stop.log", 5, stop=ready.exists)
        self.assertEqual(result.stopped, "stop requested")
        leader, descendant = json.loads(pids.read_text())
        self.assert_process_inactive(descendant)
        with self.assertRaises(ChildProcessError):
            os.waitpid(leader, os.WNOHANG)

    def test_natural_leader_exit_still_terminates_descendants(self):
        script, ready, pids = self.descendant_script(leader_exits=True)
        result = run_process([sys.executable, str(script)], self.repo,
                             self.root / "exit.log", 5)
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(result.stopped)
        self.assertTrue(ready.exists())
        leader, descendant = json.loads(pids.read_text())
        self.assert_process_inactive(descendant)
        with self.assertRaises(ChildProcessError):
            os.waitpid(leader, os.WNOHANG)

    def test_timeout_stops_a_child_without_waiting_for_its_sleep(self):
        result = run_process([sys.executable, "-c", "import time; time.sleep(60)"],
                             self.repo, self.root / "deadline.log", 0.2)
        self.assertEqual(result.stopped, "deadline exceeded")
        self.assertLess(result.elapsed, 5)


if __name__ == "__main__":
    unittest.main()
