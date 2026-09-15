"""Regression fixtures for discovery metadata and maintained guidance links."""

from pathlib import Path
import tempfile
import unittest

from agent_loop.guidance import markdown_anchors, validate


class GuidanceTests(unittest.TestCase):
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

    def test_valid_skill_agent_and_script_links(self):
        self.write("scripts/check.py", "print('check')\n")
        self.write(".agents/skills/research/SKILL.md", """---
name: research
description: >-
  Research changing API decisions using official sources.
  Use before dependency decisions.
---
[Tool](../../../scripts/check.py)
""")
        self.write(".codex/agents/reviewer.toml", '''name = "reviewer"
description = "Inspect implementation evidence."
developer_instructions = """
Read [rules](../../AGENTS.md#project).
The word model and reasoning_effort in prose is harmless.
"""
''')
        self.assertEqual(validate(self.root), [])

    def test_missing_local_target_reports_source_line(self):
        self.write("docs/development/runbook.md", "# Runbook\n\n[Tool](../../scripts/missing.py)\n")
        message = self.messages()
        self.assertIn("runbook.md:3:", message)
        self.assertIn("missing local link target", message)

    def test_broken_anchor_and_nested_scoped_guide(self):
        self.write("crates/core/AGENTS.md", "[Rules](../../AGENTS.md#absent)\n")
        self.assertIn("missing Markdown anchor", self.messages())

    def test_scoped_contracts_and_product_architecture_links_are_checked(self):
        self.write("crates/app/docs/lifetime.md", "[Owner](../src/missing.rs)\n")
        self.write("docs/architecture.md", "[Contract](missing.md)\n")
        message = self.messages()
        self.assertIn("lifetime.md:1: missing local link target", message)
        self.assertIn("architecture.md:1: missing local link target", message)

    def test_generated_run_checkouts_do_not_become_source_guidance(self):
        self.write(".local/agent-loop/fixture/accepted/AGENTS.md", "[Old](absent.md)\n")
        self.write("target/fixture/AGENTS.md", "[Generated](missing.md)\n")
        self.assertEqual(validate(self.root), [])

    def test_fenced_and_inline_examples_and_external_urls_are_ignored(self):
        self.write("AGENTS.md", """# Project
```markdown
[example](missing.md)
```
~~~
[another](also-missing.md)
~~~
`[inline](not-a-file.md)`
[external](https://example.invalid/missing#anchor)
[mail](mailto:example@example.invalid)
Prose mentions scripts/not-a-command.py and maximum effort.
""")
        self.assertEqual(validate(self.root), [])

    def test_reference_angle_encoded_and_parenthesized_links(self):
        self.write("docs/a file.md", "# First\n# First\n")
        self.write("docs/file(test).md", "Title\n=====\n")
        self.write("AGENTS.md", """# Project
[one][guide]
[guide]: <docs/a file.md#first-1> "Title"
[two](docs/a%20file.md#first)
[three](docs/file(test).md#title)
""")
        self.assertEqual(validate(self.root), [])

    def test_reference_definition_checks_missing_target(self):
        self.write("AGENTS.md", "# Project\n[guide]: missing.md\n[read][guide]\n")
        self.assertIn("missing local link target: missing.md", self.messages())

    def test_skill_name_mismatch_and_nonstring_description(self):
        self.write(".agents/skills/research/SKILL.md", "---\nname: another\ndescription: false\n---\n")
        message = self.messages()
        self.assertIn("skill name must match its directory", message)
        self.assertIn("skill field must be a string: description", message)

    def test_missing_and_duplicate_skill_fields(self):
        self.write(".agents/skills/research/SKILL.md", "---\nname: research\nname: research\n---\n")
        message = self.messages()
        self.assertIn("duplicate skill field: name", message)
        self.assertIn("skill requires a nonempty description", message)

    def test_quoted_frontmatter_and_reference_document_are_checked(self):
        self.write(".agents/skills/research/SKILL.md", "---\nname: 'research'\ndescription: \"Research current APIs.\"\n---\n")
        self.write(".agents/skills/research/references/sources.md", "[Broken](missing.md)\n")
        self.assertIn("sources.md:1: missing local link target", self.messages())

    def test_agent_requires_strings_and_inherits_settings(self):
        self.write(".codex/agents/reviewer.toml", '''name = "reviewer"
description = "Review."
developer_instructions = 123
model = "a-model"
model_reasoning_effort = "high"
''')
        message = self.messages()
        self.assertIn("agent requires a nonempty string developer_instructions", message)
        self.assertIn("agent model must be omitted", message)
        self.assertIn("agent model_reasoning_effort must be omitted", message)

    def test_duplicate_agent_names_and_invalid_toml(self):
        agent = 'name = "reviewer"\ndescription = "Review."\ndeveloper_instructions = "Read."\n'
        self.write(".codex/agents/one.toml", agent)
        self.write(".codex/agents/two.toml", agent)
        self.write(".codex/agents/invalid.toml", "name = [")
        message = self.messages()
        self.assertIn("duplicate agent name", message)
        self.assertIn("invalid agent TOML", message)

    def test_agent_link_reports_toml_source_line(self):
        self.write(".codex/agents/reviewer.toml", '''name = "reviewer"
description = "Review."
developer_instructions = """
Read [rules](../../absent.md).
"""
''')
        self.assertIn("reviewer.toml:4: missing local link target", self.messages())

    def test_link_cannot_escape_the_repository(self):
        self.write("AGENTS.md", "# Project\n[Outside](../outside.md)\n")
        self.assertIn("local link escapes repository", self.messages())

    def test_anchor_slug_formatting_unicode_and_duplicates(self):
        anchors = markdown_anchors("# A `code` & **thing**!\n# Café\n# Café\n"
                                   '<a id="custom"></a>\n`<a id="example"></a>`\n')
        self.assertEqual(anchors, {"a-code--thing", "café", "café-1", "custom"})


if __name__ == "__main__":
    unittest.main()
