"""Structural checks for the optional Claude Code configuration tree."""

from pathlib import Path
import tempfile
import unittest

from agent_loop.guidance import validate


class ClaudeGuidanceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.write("AGENTS.md", "# Project\n")
        self.write("CONTRIBUTING.md", "[Rules](AGENTS.md#project)\n")
        self.write("docs/agent-guidance.md", "# Guidance\n")

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def messages(self):
        return "\n".join(str(issue) for issue in validate(self.root))

    def valid_tree(self):
        self.write("CLAUDE.md", "@AGENTS.md\n[Guide](docs/agent-guidance.md)\n")
        self.write(".claude/agents/verifier.md", """---
name: verifier
description: Verify one candidate.
model: opus
effort: high
tools: Read, Grep, Glob, Bash
disallowedTools:
  - Edit
  - Write
permissionMode: dontAsk
maxTurns: 120
memory: project
---
Read [rules](../../AGENTS.md#project).
""")
        self.write(".claude/agents/Explore.md", "---\nname: Explore\ndescription: Cheap search.\nmodel: sonnet\nomitClaudeMd: true\n---\nSearch.\n")
        self.write(".claude/rules/commits.md", "[Contributing](../../CONTRIBUTING.md)\n")
        self.write(".claude/hooks/stop_gate.py", "import sys\nsys.exit(0)\n")
        self.write(".claude/settings.json", '{"permissions": {"deny": ["Bash(git push *)"]}, "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "python3 \\"${CLAUDE_PROJECT_DIR}/.claude/hooks/stop_gate.py\\""}]}]}}')
        self.write(".agents/skills/research/SKILL.md", "---\nname: research\ndescription: Research.\n---\n[Guide](../../../docs/agent-guidance.md)\n")
        (self.root / ".claude/skills").mkdir()
        (self.root / ".claude/skills/research").symlink_to("../../.agents/skills/research")
        self.write(".claude/skills/gates/SKILL.md", "---\nname: gates\ndescription: Run gates.\n---\n[Missing](absent.md)\n")

    def test_valid_claude_tree_passes_except_real_broken_links(self):
        self.valid_tree()
        message = self.messages()
        self.assertIn("gates/SKILL.md:5: missing local link target", message)
        self.assertEqual(len(validate(self.root)), 1)

    def test_agent_metadata_problems_are_reported(self):
        self.valid_tree()
        self.write(".claude/agents/verifier.md", "---\nname: reviewer\ndescription: Verify.\nmodel: gpt\neffort: ultra\npermissionMode: yolo\nmaxTurns: many\ntools:\n---\n")
        message = self.messages()
        for expected in ("agent name must match its file name", "agent model must be", "agent effort must be one of",
                         "agent permissionMode must be one of", "agent maxTurns must be a positive integer",
                         "agent tools must be a nonempty list", "agent requires instructions"):
            self.assertIn(expected, message)
        self.write(".claude/agents/Bad-Name.md", "---\nname: Bad-Name\ndescription: x\n---\nBody\n")
        self.assertIn("lowercase letters, digits, and hyphens", self.messages())

    def test_settings_and_hook_problems_are_reported(self):
        self.valid_tree()
        self.write(".claude/settings.json", "{not json")
        self.assertIn("invalid settings JSON", self.messages())
        self.write(".claude/settings.json", '{"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "python3 ${CLAUDE_PROJECT_DIR}/.claude/hooks/missing.py"}]}]}, "permissions": {"deny": "Bash"}}')
        message = self.messages()
        self.assertIn("hook command references a missing file: .claude/hooks/missing.py", message)
        self.assertIn("permissions.deny must be a list", message)
        self.write(".claude/hooks/stop_gate.py", "def broken(:\n")
        self.assertIn("hook does not parse", self.messages())

    def test_symlinked_skill_metadata_is_checked_once(self):
        self.valid_tree()
        self.write(".agents/skills/research/SKILL.md", "---\nname: other\ndescription: Research.\n---\n")
        message = self.messages()
        self.assertEqual(message.count("skill name must match its directory"), 2)  # both discovery paths report it
        self.assertEqual(message.count("missing local link target"), 1)


if __name__ == "__main__":
    unittest.main()
