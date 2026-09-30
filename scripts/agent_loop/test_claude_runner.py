"""Controller behavior specific to the Claude Code session tool."""

from __future__ import annotations

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch

from agent_loop import test_runner as fixtures
from agent_loop.claude import UsageLimited, resolve_selection
from agent_loop.git import git
from agent_loop.process import LoopError, read_json
from agent_loop.runner import Runner, controlled, create_run, main, make_adapter
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule
from agent_loop.test_claude_process import light_task
from agent_loop.test_codex_process import example_task


class FakeClaude(fixtures.FakeCodex):
    branch_prefix = "claude/agent-"

    def __init__(self, *args, limit_role=None, **kwargs):
        super().__init__(*args, **kwargs)
        self.limit_role = limit_role
        self.selections = []

    def configure_attempt(self, task, attempt):
        selection = {"agent": "implementer", "model": "opus", "effort": "high" if attempt == 1 else "xhigh", "max_turns": 200}
        self.selections.append((task.id, attempt))
        return selection

    def recover_usage(self, attempts):
        return 0, False

    def run(self, role, feature, *args, **kwargs):
        if role == self.limit_role:
            self.calls.append((role, feature.id))
            raise UsageLimited("implementer session was refused by a usage or rate limit; resume after the window resets")
        return super().run(role, feature, *args, **kwargs)


class ClaudeRunnerTests(unittest.TestCase):
    setUp = fixtures.RunnerTests.setUp
    prepare = fixtures.RunnerTests.prepare
    execute = fixtures.RunnerTests.execute

    def create(self, tasks=None, **overrides):
        specification = self.prepare(tasks)
        options = self.options | {"tool": "claude", "model": "claude-opus-5-5", "effort": "high", "hard_model": None,
                                  "light_model": None, "hard_effort": None, "light_effort": None, "review_effort": None,
                                  "max_turns": 200, "review_max_turns": 120, "sandbox": "off", "retry_effort": None} | overrides
        with patch("agent_loop.runner.Claude.preflight", return_value="2.1.274 (Claude Code)"):
            return create_run(self.root, specification, fixtures.CONTROLLER, options)

    def test_claude_run_snapshots_configuration_and_records_routing(self):
        directory = self.create()
        state = read_json(directory / "state.json")
        self.assertEqual(state["tool"], "claude")
        # Steps without their own selection record the base model and effort they run on.
        self.assertEqual({key: state[key] for key in ("retry_effort", "hard_model", "light_model", "hard_effort", "light_effort", "review_effort")},
                         {"retry_effort": "xhigh", "hard_model": "claude-opus-5-5", "light_model": "claude-opus-5-5",
                          "hard_effort": "xhigh", "light_effort": "medium", "review_effort": "high"})
        self.assertEqual(state["sandbox"], "off")
        self.assertIs(state["sandbox_enabled"], False)
        self.assertIn(".claude/agents/implementer.md", state["controller_files"])
        self.assertIn("scripts/agent_loop/claude-settings.json", state["controller_files"])
        self.assertNotIn(".codex/agents/implementer.toml", state["controller_files"])
        effective = read_json(directory / "controller/claude-settings.json")
        self.assertEqual(effective["env"]["GITTURTLE_LOOP"], "1")
        self.assertIn("Bash(git push *)", effective["permissions"]["deny"])
        self.assertEqual(state["claude_settings_sha256"], make_adapter(directory / "controller", state).settings_sha256)
        branch = git(directory / "accepted", "symbolic-ref", "--short", "HEAD").strip()
        self.assertTrue(branch.startswith("claude/agent-"), branch)
        adapter = FakeClaude()
        result = self.execute(directory, adapter)
        self.assertEqual(result["phase"], "complete")
        self.assertEqual(result["tool_version"], "fixture Codex")
        self.assertNotIn("codex_version", result)
        self.assertEqual(adapter.selections, [("one", 1)])
        self.assertEqual(result["tasks"]["one"]["session"]["effort"], "high")

    def test_cli_defaults_route_every_step_to_the_base_model(self):
        specification = self.prepare()
        base = ["run", "--repo", str(self.root), "--tasks", str(specification), "--tool", "claude", "--sandbox", "off",
                "--model", "claude-opus-5-5", "--effort", "high", "--max-tasks", "1", "--max-attempts", "3", "--max-minutes", "1"]
        states = []
        for extra in ([], ["--hard-model", "none", "--light-model", "sonnet", "--light-effort", "low", "--review-effort", "max"]):
            output = io.StringIO()
            with patch("agent_loop.runner.Claude.preflight", return_value="2.1.285 (Claude Code)"), \
                    patch.object(Runner, "execute", lambda runner: runner.state | {"phase": "complete"}), redirect_stdout(output):
                self.assertEqual(main(base + extra), 0, output.getvalue())
            states.append(read_json(Path(output.getvalue().splitlines()[0].removeprefix("run: ")) / "state.json"))
        # The run records concrete efforts; nothing is left for a later resume to default.
        self.assertEqual({key: states[0][key] for key in ("retry_effort", "hard_effort", "light_effort", "review_effort")},
                         {"retry_effort": "xhigh", "hard_effort": "xhigh", "light_effort": "medium", "review_effort": "high"})
        adapter = make_adapter(Path("."), states[0])
        self.assertEqual([(s["agent"], s["model"], s["effort"]) for s in (adapter.configure_attempt(light_task(), 1),
                          *(adapter.configure_attempt(example_task(), attempt) for attempt in (1, 2, 3)))],
                         [("implementer", "claude-opus-5-5", "medium"), ("implementer", "claude-opus-5-5", "high"),
                          ("implementer", "claude-opus-5-5", "xhigh"), ("implementer-hard", "claude-opus-5-5", "xhigh")])
        self.assertEqual(adapter.review_selection()["effort"], "high")
        self.assertEqual((states[1]["hard_model"], states[1]["light_model"], states[1]["review_effort"]), ("none", "sonnet", "max"))
        adapter = make_adapter(Path("."), states[1])
        self.assertEqual(adapter.configure_attempt(example_task(), 3)["agent"], "implementer")
        self.assertEqual((adapter.configure_attempt(light_task(), 1)["model"], adapter.configure_attempt(light_task(), 1)["effort"]),
                         ("sonnet", "low"))
        self.assertEqual(adapter.review_selection()["effort"], "max")

    def test_usage_limit_pauses_the_run_and_resumes_the_same_task(self):
        directory = self.create([fixtures.task(), fixtures.task("two")])
        adapter = FakeClaude(limit_role="implementer")
        state = self.execute(directory, adapter)
        self.assertEqual(state["phase"], "paused")
        self.assertIn("usage or rate limit", state["reason"])
        self.assertEqual(adapter.calls, [("implementer", "one")])
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "interrupted")
        self.assertEqual(record["attempts"], 1)
        self.assertEqual(state["tasks"]["two"]["attempts"], 0)
        resumed = FakeClaude()
        state = self.execute(directory, resumed)
        self.assertEqual(state["phase"], "complete")
        self.assertEqual(state["tasks"]["one"]["status"], "accepted")
        self.assertEqual(state["tasks"]["one"]["attempts"], 2)

    def test_usage_limit_during_review_keeps_the_candidate(self):
        directory = self.create()
        state = self.execute(directory, FakeClaude(limit_role="verifier"))
        self.assertEqual(state["phase"], "paused")
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "review_blocked")
        self.assertIn("candidate", record)
        state = self.execute(directory, FakeClaude())
        self.assertEqual(state["tasks"]["one"]["status"], "accepted")
        self.assertEqual(state["tasks"]["one"]["attempts"], 1)

    def test_adapter_selection_defaults_to_codex_and_rejects_unknown_tools(self):
        self.assertEqual(type(make_adapter(Path("."), {"model": "m", "effort": "high"})).__name__, "Codex")
        resolved = resolve_selection("m", "high", {})
        self.assertEqual(type(make_adapter(Path("."), {"model": "m", "effort": "high", "tool": "claude"} | resolved)).__name__, "Claude")
        # Only a run this controller created reaches make_adapter, and it records every step.
        with self.assertRaisesRegex(LoopError, "effort must be one of"):
            make_adapter(Path("."), {"model": "m", "effort": "high", "tool": "claude"})
        with self.assertRaisesRegex(LoopError, "unknown session tool"):
            make_adapter(Path("."), {"model": "m", "effort": "high", "tool": "cursor"})
        with self.assertRaisesRegex(LoopError, "unknown session tool"):
            self.create(tool="cursor")

    def test_claude_configuration_is_protected_in_every_run(self):
        for path in (".claude/settings.json", ".claude/agents/verifier.md", "crates/app/CLAUDE.md", "CLAUDE.md", ".codex/agents/x.toml"):
            self.assertTrue(controlled(path), path)
        self.assertFalse(controlled("crates/app/src/main.rs"))


if __name__ == "__main__":
    unittest.main()
