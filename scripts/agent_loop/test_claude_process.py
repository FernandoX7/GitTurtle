"""Exercise the Claude Code CLI contract with a fake executable and disposable processes."""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from agent_loop.claude import (
    CHILD_ENVIRONMENT, REVIEW_ALLOWED, Claude, UsageLimited, effort_above, parse_version,
    snapshot_files,
)
from agent_loop.codex import validate_review
from agent_loop.process import EnvironmentBlocked, LoopError, MalformedResponse, atomic_json, digest
from agent_loop.task_spec import Task
from agent_loop.test_codex_process import example_task, passing_review
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


AGENTS = ("implementer", "implementer-hard", "verifier", "security-reviewer")


def light_task() -> Task:
    return Task(
        id="explain-loop", title="Explain the loop", description="Document the controller.",
        depends_on=(), scope=("docs/**",), acceptance=({"id": "content", "description": "The guide is accurate."},),
        profiles=("docs",), commit="docs: explain the controller loop",
    )


@unittest.skipUnless(os.name == "posix", "The controller supports macOS and Linux processes")
class ClaudeProcessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "checkout"
        self.controller = self.root / "run" / "controller"
        for base in (self.repo, self.controller):
            agents = base / ".claude" / "agents"
            agents.mkdir(parents=True)
            for agent in AGENTS:
                (agents / f"{agent}.md").write_text(
                    f"---\nname: {agent}\ndescription: Fixture role.\n---\nInspect the fixture contract.\n",
                    encoding="utf-8",
                )
        atomic_json(self.controller / "claude-settings.json", {"env": dict(CHILD_ENVIRONMENT)})
        self.task = example_task()
        self.adapter = Claude(self.controller, "selected-model", "medium",
                              settings_sha256=digest(self.controller / "claude-settings.json"))
        self.invocation = self.root / "invocation.json"

    def fake_claude(self, *, result=None, structured=None, exitcode=0, stderr="", version="2.1.274 (Claude Code)",
                    help_flags=None, logged_in=True, raw_stdout=None):
        if structured is None:
            structured = {"task_id": self.task.id, "status": "ready", "summary": "Fixture change is ready."}
        payload = {
            "type": "result", "subtype": "success", "is_error": False, "result": json.dumps(structured),
            "structured_output": structured, "session_id": "fixture-session", "num_turns": 3,
            "usage": {"output_tokens": 37, "input_tokens": 5}, "total_cost_usd": 0.5,
            "modelUsage": {"selected-model": {"outputTokens": 37}}, "permission_denials": [], "duration_ms": 10,
        }
        if result:
            payload.update(result)
        if help_flags is None:
            help_flags = ("--agent --json-schema --permission-mode --permission-prompts --settings --strict-mcp-config "
                          "--output-format --effort --model --tools --disallowedTools --allowedTools")
        config = {"payload": payload, "exitcode": exitcode, "stderr": stderr, "version": version,
                  "help": help_flags, "logged_in": logged_in, "raw_stdout": raw_stdout}
        executable = self.root / "fake-claude"
        executable.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\nfrom pathlib import Path\n"
            f"config = {config!r}\n"
            "if sys.argv[1:] == ['--version']:\n    print(config['version']); raise SystemExit(0)\n"
            "if sys.argv[1:] == ['--help']:\n    print(config['help']); raise SystemExit(0)\n"
            "if sys.argv[1:] == ['auth', 'status']:\n"
            "    print(json.dumps({'loggedIn': config['logged_in'], 'subscriptionType': 'max'})); raise SystemExit(0)\n"
            "args = sys.argv[1:]\nprompt = sys.stdin.read()\n"
            "keys = ['GITTURTLE_LOOP', 'GITTURTLE_TASK_CONTEXT', 'GITTURTLE_TASKS_PATH', 'CARGO_TARGET_DIR',\n"
            "        'HOME', 'PATH', 'CLAUDE_CODE_DISABLE_BACKGROUND_TASKS',\n"
            "        'CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS', 'INSTA_UPDATE']\n"
            "capture = {'args': args, 'prompt': prompt, 'cwd': os.getcwd(),\n"
            "           'environment': {k: os.environ[k] for k in keys if k in os.environ}}\n"
            f"Path({str(self.invocation)!r}).write_text(json.dumps(capture))\n"
            "if config['stderr']:\n    print(config['stderr'], file=sys.stderr)\n"
            "if config['raw_stdout'] is not None:\n    sys.stdout.write(config['raw_stdout'])\n"
            "else:\n    print(json.dumps(config['payload']))\n"
            "raise SystemExit(config['exitcode'])\n", encoding="utf-8"
        )
        executable.chmod(0o755)
        self.adapter.executable = str(executable)
        return executable

    def session(self, role="implementer", directory="attempt", **options):
        return self.adapter.run(role, self.task, self.repo, self.root / directory, 5, lambda: False, **options)

    def capture(self) -> dict:
        return json.loads(self.invocation.read_text())

    def test_implementer_cli_contract_environment_and_evidence_files(self):
        self.fake_claude()
        value = self.session(context="Gate evidence: fixture-check.log")
        self.assertEqual(value["status"], "ready")
        self.assertEqual(self.adapter.output_tokens, 37)
        capture = self.capture()
        args = capture["args"]
        self.assertEqual(args[0], "-p")
        for flag, expected in (("--agent", "implementer"), ("--model", "selected-model"), ("--effort", "medium"),
                               ("--permission-mode", "bypassPermissions"), ("--permission-prompts", "none"),
                               ("--max-turns", "200"), ("--output-format", "json"),
                               ("--settings", str(self.controller / "claude-settings.json"))):
            self.assertEqual(args[args.index(flag) + 1], expected, flag)
        self.assertIn("--strict-mcp-config", args)
        self.assertNotIn("--tools", args)
        self.assertNotIn("--bare", args)
        self.assertNotIn("--dangerously-skip-permissions", args)
        schema = json.loads(args[args.index("--json-schema") + 1])
        self.assertEqual(schema["required"], ["task_id", "status", "summary"])
        environment = capture["environment"]
        self.assertEqual(environment["GITTURTLE_LOOP"], "1")
        self.assertEqual(environment["INSTA_UPDATE"], "no")
        self.assertEqual(environment["CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS"], "4")
        self.assertEqual(environment["CARGO_TARGET_DIR"], str(self.controller.parent / "build"))
        self.assertEqual(environment["HOME"], os.environ["HOME"])
        self.assertEqual(environment["PATH"], os.environ["PATH"])
        self.assertTrue(Path(environment["GITTURTLE_TASK_CONTEXT"]).is_file())
        self.assertEqual(Path(capture["cwd"]).resolve(), self.repo.resolve())
        self.assertIn('"id": "inspect-history"', capture["prompt"])
        self.assertIn("fixture-check.log", capture["prompt"])
        attempt = self.root / "attempt"
        self.assertEqual(capture["prompt"], (attempt / "implementer.prompt.txt").read_text())
        for name in ("implementer.process.json", "implementer.session.json", "implementer.response.json",
                     "implementer.schema.json", "implementer.contract.json", "implementer.stdout.log"):
            self.assertTrue((attempt / name).is_file(), name)
        session = json.loads((attempt / "implementer.session.json").read_text())
        self.assertEqual(session["session_id"], "fixture-session")
        self.assertEqual(session["output_tokens"], 37)

    def test_active_task_queue_reaches_the_session_only_when_supplied(self):
        """The hook protects the run's own queue, so the session must name it."""
        self.fake_claude()
        with patch.dict(os.environ):
            os.environ.pop("GITTURTLE_TASKS_PATH", None)
            self.session(directory="queued", spec_path="docs/development/themes/tasks.json")
            queued = self.capture()["environment"]
            self.session(directory="unqueued")
            plain = self.capture()["environment"]
        self.assertEqual(queued["GITTURTLE_TASKS_PATH"], "docs/development/themes/tasks.json")
        self.assertEqual(queued["GITTURTLE_LOOP"], "1")
        self.assertNotIn("GITTURTLE_TASKS_PATH", plain)

    def test_verifier_runs_read_only_with_allowlisted_commands(self):
        self.fake_claude(structured=passing_review())
        result = self.session("verifier", candidate="a" * 40)
        args = self.capture()["args"]
        self.assertEqual(args[args.index("--agent") + 1], "verifier")
        self.assertEqual(args[args.index("--permission-mode") + 1], "dontAsk")
        self.assertEqual(args[args.index("--max-turns") + 1], "120")
        self.assertEqual(args[args.index("--tools") + 1], "Read,Grep,Glob,Bash")
        self.assertEqual(args[args.index("--disallowedTools") + 1], "Edit,Write,NotebookEdit,Agent")
        self.assertEqual(args[args.index("--allowedTools") + 1:], list(REVIEW_ALLOWED))
        self.assertIn("Candidate: " + "a" * 40, self.capture()["prompt"])
        self.assertEqual(validate_review(result, self.task, "a" * 40), "pass")

    def test_every_role_is_given_its_schema_field_names_verbatim(self):
        # Told only that "a schema was supplied", a session invents its own field
        # names and the CLI then returns no structured output at all.
        build = {"task_id": self.task.id, "status": "ready", "summary": "Fixture change is ready."}
        for role, options, response, fields in (
            ("implementer", {}, build, ("task_id", "status", "summary")),
            ("verifier", {"candidate": "a" * 40}, passing_review(),
             ("task_id", "candidate", "verdict", "criteria", "findings")),
        ):
            with self.subTest(role=role):
                self.fake_claude(structured=response)
                self.session(role, directory=f"schema-{role}", **options)
                capture = self.capture()
                prompt = capture["prompt"]
                schema = json.loads(capture["args"][capture["args"].index("--json-schema") + 1])
                self.assertIn(json.dumps(schema, indent=2), prompt)
                for field in fields:
                    self.assertIn(f'"{field}"', prompt)

    def test_an_unreadable_verdict_is_reported_as_a_malformed_response(self):
        # The gated candidate it judged is intact, so the runner retries the
        # review rather than spending an attempt rebuilding it.
        self.fake_claude(result={"structured_output": None})
        with self.assertRaisesRegex(MalformedResponse, "no structured output"):
            self.session("verifier", candidate="a" * 40)

    def test_security_reviewer_uses_its_schema_agent_and_explicit_base(self):
        from agent_loop.test_security_review import passing_security
        response = passing_security(self.task.id, "b" * 40, "a" * 40)
        self.fake_claude(structured=response)
        result = self.session("security-reviewer", candidate="a" * 40, base="b" * 40)
        self.assertEqual(result, response)
        args = self.capture()["args"]
        self.assertEqual(args[args.index("--agent") + 1], "security-reviewer")
        schema = json.loads(args[args.index("--json-schema") + 1])
        self.assertIn("coverage", schema["required"])
        self.assertIn("Base: " + "b" * 40, self.capture()["prompt"])

    def test_missing_review_identity_never_launches(self):
        self.fake_claude(structured=passing_review())
        for role, options in (("verifier", {}), ("security-reviewer", {"candidate": "a" * 40}), ("security-reviewer", {"base": "b" * 40})):
            with self.subTest(role=role, options=options), self.assertRaisesRegex(LoopError, "explicit"):
                self.session(role, **options)
        self.assertFalse(self.invocation.exists())

    def test_failed_session_is_a_capability_pause_and_turn_exhaustion_is_a_failure(self):
        self.fake_claude(result={"is_error": True, "subtype": "error_during_execution", "result": "boom"})
        with self.assertRaisesRegex(EnvironmentBlocked, "session failed"):
            self.session(directory="failed")
        self.fake_claude(result={"subtype": "error_max_turns", "is_error": True, "result": "ran out"})
        with self.assertRaisesRegex(LoopError, "turn limit"):
            self.session(directory="turns")
        self.assertTrue(self.adapter.output_tokens >= 0)

    def test_usage_limit_text_pauses_instead_of_failing(self):
        self.fake_claude(result={"is_error": True, "subtype": "error_during_execution",
                                 "result": "You've hit your session limit. Try again at 3pm."})
        with self.assertRaises(UsageLimited):
            self.session(directory="limited-json")
        self.fake_claude(raw_stdout="", exitcode=1, stderr="Rate limit reached for this organization")
        with self.assertRaises(UsageLimited):
            self.session(directory="limited-stderr")
        self.assertTrue(self.adapter.output_usage_incomplete)
        self.fake_claude(raw_stdout="not json at all\n", exitcode=1, stderr="segfault")
        with self.assertRaisesRegex(EnvironmentBlocked, "no result record"):
            self.session(directory="crash")

    def test_missing_structured_output_or_malformed_status_cannot_pass(self):
        self.fake_claude(result={"structured_output": None})
        with self.assertRaisesRegex(LoopError, "structured output"):
            self.session(directory="unstructured")
        self.fake_claude(structured={"task_id": self.task.id, "status": [], "summary": "Invalid enum."})
        with self.assertRaises(LoopError):
            self.session(directory="malformed")
        self.fake_claude(structured={"task_id": self.task.id, "status": "blocked", "summary": "Native fixture unavailable."})
        self.assertEqual(self.session(directory="blocked")["status"], "blocked")

    def test_prior_output_is_preserved_without_launching_another_session(self):
        self.fake_claude()
        directory = self.root / "attempt"
        directory.mkdir()
        output = directory / "implementer.response.json"
        output.write_text('{"task_id":"inspect-history","status":"ready","summary":"old"}')
        with self.assertRaisesRegex(LoopError, "output already exists"):
            self.session()
        self.assertFalse(self.invocation.exists())

    def test_agent_definition_must_match_the_run_snapshot(self):
        self.fake_claude()
        live = self.repo / ".claude/agents/implementer.md"
        live.write_text(live.read_text() + "Edited after the run started.\n")
        with self.assertRaisesRegex(LoopError, "differs from the run snapshot"):
            self.session(directory="drift")
        live.unlink()
        with self.assertRaisesRegex(LoopError, "no Claude agent definition"):
            self.session(directory="missing")
        self.assertFalse(self.invocation.exists())

    def test_changed_settings_snapshot_refuses_to_launch(self):
        self.fake_claude()
        atomic_json(self.controller / "claude-settings.json", {"env": {}, "changed": True})
        with self.assertRaisesRegex(LoopError, "settings snapshot"):
            self.session()
        self.assertFalse(self.invocation.exists())

    def test_preflight_checks_version_flags_sign_in_and_roles_without_a_session(self):
        self.fake_claude()
        self.assertEqual(self.adapter.preflight(), "2.1.274 (Claude Code)")
        self.assertFalse(self.invocation.exists())
        self.fake_claude(version="2.1.100 (Claude Code)")
        with self.assertRaisesRegex(EnvironmentBlocked, "older than"):
            self.adapter.preflight()
        self.fake_claude(help_flags="--agent --model")
        with self.assertRaisesRegex(EnvironmentBlocked, "required flag"):
            self.adapter.preflight()
        self.fake_claude(logged_in=False)
        with self.assertRaisesRegex(EnvironmentBlocked, "not signed in"):
            self.adapter.preflight()
        self.fake_claude()
        (self.controller / ".claude/agents/implementer-hard.md").unlink()
        with self.assertRaisesRegex(EnvironmentBlocked, "missing Claude agent definition"):
            self.adapter.preflight()
        self.adapter.hard_model = None
        self.assertEqual(self.adapter.preflight(), "2.1.274 (Claude Code)")
        self.adapter.sandbox = "on"
        with patch.object(Claude, "sandbox_available", return_value=False):
            with self.assertRaisesRegex(EnvironmentBlocked, "sandbox requested"):
                self.adapter.preflight()
        self.assertFalse(self.invocation.exists())

    def test_attempt_routing_by_attempt_number_and_declared_profiles(self):
        adapter = Claude(self.controller, "opus", "high")
        expect = [("implementer", "opus", "high"), ("implementer", "opus", "xhigh"), ("implementer-hard", "fable", "high"),
                  ("implementer-hard", "fable", "high")]
        for attempt, (agent, model, effort) in enumerate(expect, start=1):
            with self.subTest(attempt=attempt):
                selection = adapter.configure_attempt(self.task, attempt)
                self.assertEqual((selection["agent"], selection["model"], selection["effort"], selection["max_turns"]),
                                 (agent, model, effort, 200))
        light = [("implementer", "sonnet", "medium"), ("implementer", "opus", "high"), ("implementer-hard", "fable", "high")]
        for attempt, (agent, model, effort) in enumerate(light, start=1):
            with self.subTest(light=attempt):
                selection = adapter.configure_attempt(light_task(), attempt)
                self.assertEqual((selection["agent"], selection["model"], selection["effort"]), (agent, model, effort))
        plain = Claude(self.controller, "opus", "max", hard_model="none", light_model="none", retry_effort="low")
        self.assertEqual(plain.configure_attempt(light_task(), 1)["model"], "opus")
        self.assertEqual(plain.configure_attempt(self.task, 2)["effort"], "low")
        self.assertEqual(plain.configure_attempt(self.task, 3), {"agent": "implementer", "model": "opus", "effort": "low", "max_turns": 200})
        self.assertEqual(effort_above("max"), "max")
        self.assertEqual(effort_above("low"), "medium")
        self.assertEqual(parse_version("2.1.274 (Claude Code)"), (2, 1, 274))
        with self.assertRaises(LoopError):
            Claude(self.controller, "opus", "ultra")

    def test_selection_is_consumed_by_the_next_implementer_session(self):
        self.fake_claude()
        self.adapter.configure_attempt(self.task, 3)
        self.session(directory="hard")
        args = self.capture()["args"]
        self.assertEqual(args[args.index("--agent") + 1], "implementer-hard")
        self.assertEqual(args[args.index("--model") + 1], "fable")
        self.fake_claude(structured=passing_review())
        self.session("verifier", directory="review", candidate="a" * 40)
        args = self.capture()["args"]
        self.assertEqual(args[args.index("--model") + 1], "selected-model")
        self.fake_claude()
        self.session(directory="plain")
        self.assertEqual(self.capture()["args"][2], "implementer")

    def test_recover_usage_reads_session_records(self):
        attempts = self.root / "attempts"
        for relative, tokens in (("one/1/implementer", 12), ("one/1/review-x/verifier", 30)):
            path = attempts / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.with_suffix(".prompt.txt").write_text("prompt")
            atomic_json(path.with_suffix(".session.json"), {"output_tokens": tokens})
        self.assertEqual(self.adapter.recover_usage(attempts), (42, False))
        (attempts / "one/2/implementer.prompt.txt").parent.mkdir(parents=True)
        (attempts / "one/2/implementer.prompt.txt").write_text("interrupted")
        self.assertEqual(self.adapter.recover_usage(attempts), (42, True))

    def test_prepare_run_writes_effective_settings_from_the_pinned_template(self):
        run = self.root / "run"
        template = run / "controller/scripts/agent_loop/claude-settings.json"
        atomic_json(template, {"permissions": {"deny": ["Bash(git push *)"]}, "env": {"KEEP": "1"}})
        adapter = Claude(run / "controller", "opus", "high", sandbox="off")
        prepared = adapter.prepare_run(run)
        effective = json.loads((run / "controller/claude-settings.json").read_text())
        self.assertEqual(effective["permissions"]["deny"], ["Bash(git push *)"])
        self.assertEqual(effective["env"]["KEEP"], "1")
        self.assertEqual(effective["env"]["GITTURTLE_LOOP"], "1")
        self.assertNotIn("sandbox", effective)
        self.assertEqual(prepared, {"claude_settings_sha256": digest(run / "controller/claude-settings.json"), "sandbox_enabled": False})
        adapter.sandbox = "on"
        with patch.object(Claude, "sandbox_available", return_value=True):
            prepared = adapter.prepare_run(run)
        effective = json.loads((run / "controller/claude-settings.json").read_text())
        self.assertTrue(prepared["sandbox_enabled"])
        self.assertTrue(effective["sandbox"]["enabled"])
        self.assertIn(str(run / "build"), effective["sandbox"]["filesystem"]["allowWrite"])

    def test_snapshot_follows_symlinked_skills_inside_the_controller_only(self):
        controller = self.root / "source"
        (controller / ".agents/skills/research").mkdir(parents=True)
        (controller / ".agents/skills/research/SKILL.md").write_text("---\nname: research\ndescription: Research.\n---\n")
        (controller / ".claude/skills").mkdir(parents=True)
        (controller / ".claude/skills/research").symlink_to("../../.agents/skills/research")
        (controller / ".claude/agents").mkdir()
        (controller / ".claude/agents/implementer.md").write_text("---\nname: implementer\ndescription: Role.\n---\nBody\n")
        (controller / ".claude/hooks").mkdir()
        (controller / ".claude/hooks/stop_gate.py").write_text("print('gate')\n")
        (controller / ".claude/hooks/__pycache__").mkdir()
        (controller / ".claude/hooks/__pycache__/stop_gate.cpython-312.pyc").write_bytes(b"\0")
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "SKILL.md").write_text("---\nname: outside\ndescription: Outside.\n---\n")
        (controller / ".claude/skills/outside").symlink_to(outside)
        (controller / ".claude/settings.json").write_text("{}")
        selected = dict(snapshot_files(controller))
        self.assertEqual(set(selected), {
            ".claude/agents/implementer.md", ".claude/settings.json", ".claude/hooks/stop_gate.py",
            ".claude/skills/research/SKILL.md",
        })


if __name__ == "__main__":
    unittest.main()
