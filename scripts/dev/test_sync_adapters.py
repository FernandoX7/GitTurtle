"""Tests for canonical `.agents/` adapter generation and verification."""

from __future__ import annotations

import os
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev import sync_adapters  # noqa: E402


class SyncAdaptersTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        (self.root / ".claude").mkdir()
        (self.root / ".agents/roles").mkdir(parents=True)
        (self.root / ".agents/skills/sample-skill").mkdir(parents=True)
        (self.root / ".agents/skills/sample-skill/SKILL.md").write_text(
            "---\nname: sample-skill\ndescription: Sample.\n---\nBody.\n",
            encoding="utf-8",
        )
        (self.root / ".agents/roles/code-reviewer.md").write_text(
            "---\n"
            "name: code-reviewer\n"
            "description: Review diffs.\n"
            "claude_tools: Read, Grep, Glob, Bash\n"
            "claude_disallowed_tools: Edit, Write\n"
            "claude_permission_mode: dontAsk\n"
            "codex_sandbox: read-only\n"
            "---\n"
            "Read ../../AGENTS.md before reviewing.\n",
            encoding="utf-8",
        )

    def test_write_and_check_round_trip(self):
        self.assertEqual(sync_adapters.main(["--root", str(self.root), "--write"]), 0)
        self.assertEqual(sync_adapters.check_sync(self.root), [])

        claude_agent = (self.root / ".claude/agents/code-reviewer.md").read_text(encoding="utf-8")
        self.assertIn("name: code-reviewer", claude_agent)
        self.assertNotIn("model:", claude_agent)
        self.assertNotIn("effort:", claude_agent)

        codex_data = tomllib.loads((self.root / ".codex/agents/code-reviewer.toml").read_text(encoding="utf-8"))
        self.assertEqual(codex_data["name"], "code-reviewer")
        self.assertEqual(codex_data["sandbox_mode"], "read-only")
        self.assertNotIn("model", codex_data)
        self.assertIn("AGENTS.md", codex_data["developer_instructions"])

        link = self.root / ".claude/skills/sample-skill"
        self.assertTrue(link.is_symlink())
        self.assertEqual(os.readlink(link), "../../.agents/skills/sample-skill")

    def test_rejects_pinned_model_or_drift(self):
        self.assertEqual(sync_adapters.main(["--root", str(self.root), "--write"]), 0)
        (self.root / ".claude/agents/stale.md").write_text("stale\n", encoding="utf-8")
        issues = sync_adapters.check_sync(self.root)
        self.assertTrue(any("stale.md" in item for item in issues))

        (self.root / ".agents/roles/code-reviewer.md").write_text(
            "---\nname: code-reviewer\ndescription: x\nmodel: opus\n---\nBody.\n",
            encoding="utf-8",
        )
        issues = sync_adapters.check_sync(self.root)
        self.assertTrue(any("omit 'model'" in item for item in issues))


if __name__ == "__main__":
    unittest.main()
