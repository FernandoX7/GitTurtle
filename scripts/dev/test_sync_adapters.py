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

    def test_rejects_symlink_targets_and_escapes_control_chars(self):
        self.assertEqual(sync_adapters.main(["--root", str(self.root), "--write"]), 0)
        outside = self.root / "outside.md"
        outside.write_text("original\n", encoding="utf-8")
        target = self.root / ".claude/agents/code-reviewer.md"
        target.unlink()
        target.symlink_to(outside)
        issues = sync_adapters.check_sync(self.root)
        self.assertTrue(any("must not be a symlink" in item for item in issues))
        # --write unlinks the symlink instead of overwriting the external target.
        self.assertEqual(sync_adapters.main(["--root", str(self.root), "--write"]), 0)
        self.assertFalse(target.is_symlink())
        self.assertEqual(outside.read_text(encoding="utf-8"), "original\n")
        # TOML escaping handles control characters and YAML scalar quoting handles `: `.
        role = sync_adapters.RoleSpec(
            name="demo",
            description="Review: safety # check",
            claude_tools="Read",
            claude_disallowed_tools="",
            claude_permission_mode="dontAsk",
            claude_max_turns="",
            codex_sandbox="read-only",
            body="Line 1\x07\rLine 2",
        )
        self.assertIn('description: "Review: safety # check"', sync_adapters.render_claude_agent(role))
        parsed = tomllib.loads(sync_adapters.render_codex_agent(role))
        self.assertEqual(parsed["developer_instructions"], "Line 1\x07\rLine 2\n")

    def test_parse_role_preserves_quoted_hash_and_escapes_and_rejects_unquoted_hash(self):
        quoted = self.root / ".agents/roles/code-reviewer.md"
        quoted.write_text(
            '---\nname: code-reviewer\ndescription: "Check #[gpui::test] \\"quoted\\" \\\\ paths"\n---\nBody.\n',
            encoding="utf-8",
        )
        spec = sync_adapters.parse_role(quoted)
        self.assertEqual(spec.description, 'Check #[gpui::test] "quoted" \\ paths')
        unquoted = self.root / ".agents/roles/code-reviewer.md"
        unquoted.write_text(
            "---\nname: code-reviewer\ndescription: Check #[gpui::test] unquoted\n---\nBody.\n",
            encoding="utf-8",
        )
        with self.assertRaises(ValueError):
            sync_adapters.parse_role(unquoted)


if __name__ == "__main__":
    unittest.main()
