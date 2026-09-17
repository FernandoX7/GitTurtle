"""Controller behavior specific to the Claude Code session tool."""

from __future__ import annotations

import json
from pathlib import Path
import unittest
from unittest.mock import patch

from agent_loop import test_runner as fixtures
from agent_loop.claude import UsageLimited
from agent_loop.git import git
from agent_loop.process import LoopError, read_json
from agent_loop.runner import Runner, controlled, create_run, make_adapter


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
        options = self.options | {"tool": "claude", "model": "opus", "effort": "high", "hard_model": "fable",
                                  "light_model": "sonnet", "max_turns": 200, "review_max_turns": 120,
                                  "sandbox": "off", "retry_effort": None} | overrides
        with patch("agent_loop.runner.Claude.preflight", return_value="2.1.274 (Claude Code)"):
            return create_run(self.root, specification, fixtures.CONTROLLER, options)

    def test_claude_run_snapshots_configuration_and_records_routing(self):
        directory = self.create()
        state = read_json(directory / "state.json")
        self.assertEqual(state["tool"], "claude")
        self.assertEqual(state["hard_model"], "fable")
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
        self.assertEqual(type(make_adapter(Path("."), {"model": "m", "effort": "high", "tool": "claude"})).__name__, "Claude")
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
