"""Regression fixtures for discovery metadata and maintained guidance links."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock
import zlib

from agent_loop import guidance
from agent_loop.guidance import markdown_anchors, validate


def chunk(kind: bytes, body: bytes) -> bytes:
    return len(body).to_bytes(4, "big") + kind + body + zlib.crc32(kind + body).to_bytes(4, "big")


def png(*chunks: bytes) -> bytes:
    header = chunk(b"IHDR", (1).to_bytes(4, "big") * 2 + bytes([8, 0, 0, 0, 0]))
    return guidance._PNG_SIGNATURE + header + b"".join(chunks) + chunk(b"IEND", b"")


def segment(marker: int, body: bytes) -> bytes:
    return bytes([0xFF, marker]) + (len(body) + 2).to_bytes(2, "big") + body


def jpeg(*segments: bytes, after_scan: bytes = b"") -> bytes:
    scan = segment(0xDA, bytes(8)) + b"\x12\xff\x00\x34\xff\xd0\x56"
    return b"\xff\xd8" + b"".join(segments) + scan + after_scan + b"\xff\xd9"


def exif(tag_text: bytes) -> bytes:
    """A little-endian TIFF block with one entry whose value is `tag_text`."""
    entry = (0x9C9D).to_bytes(2, "little") + (1).to_bytes(2, "little") + len(tag_text).to_bytes(4, "little")
    ifd = (1).to_bytes(2, "little") + entry + (26).to_bytes(4, "little") + bytes(4)
    return b"II*\x00" + (8).to_bytes(4, "little") + ifd + tag_text


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

    def test_home_paths_under_docs_are_rejected(self):
        self.write("docs/benchmarks/run.json", '{\n "HOME": "/home/alice/src/run/home",\n'
                   ' "cwd": "/Users/bob.smith/GitTurtle"\n}\n')
        message = self.messages()
        self.assertIn("run.json:2: absolute home path /home/alice", message)
        self.assertIn("run.json:3: absolute home path /Users/bob.smith", message)

    def test_redacted_relative_and_placeholder_home_paths_pass(self):
        self.write("docs/benchmarks/record.json", '{"log": "Compiling (/Users/REDACTED/GitTurtle)",'
                   ' "note": "referenced /Users/REDACTED.",'
                   ' "sanitized": "Checkout/home/icon-temporary paths replaced",'
                   ' "run": "<worktree>/.local/themes-evidence/run/home"}\n')
        self.write("docs/notes.md", "Paths such as `/home/<user>/…`, /home/<name>/ and /Users/<name>/ "
                   "are placeholders; files under /home/ belong to users.\n")
        # Pixel data is neither documentation text nor image metadata.
        self.write("docs/evidence/frame.png", "").write_bytes(png(chunk(b"IDAT", b"/home/alice/")))
        self.assertEqual(validate(self.root), [])

    def test_anchor_slug_formatting_unicode_and_duplicates(self):
        anchors = markdown_anchors("# A `code` & **thing**!\n# Café\n# Café\n"
                                   '<a id="custom"></a>\n`<a id="example"></a>`\n')
        self.assertEqual(anchors, {"a-code--thing", "café", "café-1", "custom"})


class ImageMetadataTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        patcher = mock.patch.dict(os.environ, {guidance.PRIVATE_STRINGS_ENV: ""})
        patcher.start()
        self.addCleanup(patcher.stop)

    def image(self, name, data):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def messages(self):
        return [f"{issue.path.relative_to(self.root)}: {issue.message}"
                for issue in guidance.image_metadata_issues(self.root)]

    def test_each_png_text_chunk_and_exif_is_parsed(self):
        cases = {
            "text.png": (png(chunk(b"tEXt", b"Source\0/home/alice/shot.png")), "PNG tEXt holds an absolute home path"),
            "ztxt.png": (png(chunk(b"zTXt", b"Author\0\0" + zlib.compress(b"alice@users.noreply.github.com"))),
                         "PNG zTXt holds an email address"),
            "itxt.png": (png(chunk(b"iTXt", b"Comment\0\0\0en\0\0C:\\Users\\alice\\shot.png")),
                         "PNG iTXt holds an absolute home path"),
            "xmp.png": (png(chunk(b"iTXt", guidance._XMP_KEYWORD + b"\0\1\0\0\0" +
                                  zlib.compress(b"<x:xmpmeta><dc:creator>alice@mail.example.co</dc:creator>"))),
                        "PNG iTXt XMP holds an email address"),
            "exif.png": (png(chunk(b"eXIf", exif("/Users/alice/Desktop".encode("utf-16-le")))),
                         "PNG eXIf holds an absolute home path"),
        }
        for name, (data, _) in cases.items():
            self.image(name, data)
        found = self.messages()
        for name, (_, expected) in cases.items():
            self.assertIn(f"{name}: image metadata {expected}; strip or rewrite that metadata", found)
        self.assertEqual(len(found), len(cases), found)

    def test_each_jpeg_segment_is_parsed_including_after_the_scan(self):
        self.image("exif.jpg", jpeg(segment(0xE1, b"Exif\0\0" + exif(b"/home/alice/"))))
        self.image("xmp.jpeg", jpeg(segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0<rdf>bob@mail.example.co</rdf>")))
        self.image("comment.jpg", jpeg(segment(0xE0, b"JFIF\0"), after_scan=segment(0xFE, b"/Users/bob/pic")))
        # Some tracked .png files hold JPEG data; the parser follows the content.
        self.image("named.png", jpeg(segment(0xFE, b"carol@mail.example.co")))
        self.assertEqual(sorted(self.messages()), sorted([
            "comment.jpg: image metadata JPEG COM holds an absolute home path; strip or rewrite that metadata",
            "exif.jpg: image metadata JPEG APP1 EXIF holds an absolute home path; strip or rewrite that metadata",
            "named.png: image metadata JPEG COM holds an email address; strip or rewrite that metadata",
            "xmp.jpeg: image metadata JPEG APP1 XMP holds an email address; strip or rewrite that metadata",
        ]))

    def test_existing_tool_icc_photoshop_and_c2pa_metadata_pass(self):
        profile = b"acsp Display P3 /home/alice/profile.icc alice@mail.example.co"
        self.image("plot.png", png(chunk(b"tEXt", b"Software\0Matplotlib version3.8.0, https://matplotlib.org/"),
                                   chunk(b"iCCP", b"P3\0\0" + zlib.compress(profile)),
                                   chunk(b"caBX", b"jumb c2pa claim_generator alice@mail.example.co"),
                                   chunk(b"IDAT", b"pixels@ab.cd /home/alice/")))
        self.image("photo.jpg", jpeg(segment(0xE2, b"ICC_PROFILE\0\1\1" + profile),
                                     segment(0xED, b"Photoshop 3.0\08BIM /Users/alice/"),
                                     segment(0xE1, b"Exif\0\0" + exif(b"Adobe Photoshop 25.0 (Macintosh)"))))
        self.assertEqual(self.messages(), [])

    def test_reserved_example_domains_and_redacted_homes_pass(self):
        text = (b"qa@example.invalid a@example.com b@mail.example.org c@example.net d@host.example "
                b"/home/REDACTED/x /Users/<user>/ Checkout/home/icon")
        self.image("ok.png", png(chunk(b"tEXt", b"Comment\0" + text)))
        self.image("icon.svg", b'<svg xmlns="http://www.w3.org/2000/svg"><!-- qa@example.invalid --></svg>')
        self.assertEqual(self.messages(), [])

    def test_svg_source_and_other_formats_are_scanned(self):
        self.image("art.svg", b'<svg inkscape:export-filename="/home/alice/art.png"/>')
        self.image("clip.gif", b"GIF89a\0\0!\xfe\x10dave@mail.example.co\0;")
        self.image("broken.png", guidance._PNG_SIGNATURE + b"truncated /Users/erin/")
        self.assertEqual(sorted(self.messages()), [
            "art.svg: image metadata SVG source holds an absolute home path; strip or rewrite that metadata",
            "broken.png: image metadata embedded bytes holds an absolute home path; strip or rewrite that metadata",
            "clip.gif: image metadata embedded bytes holds an email address; strip or rewrite that metadata",
        ])

    def test_configured_private_strings_are_reported_but_never_echoed(self):
        strings = self.root.parent / f"{self.root.name}-private.txt"
        self.addCleanup(strings.unlink, missing_ok=True)
        strings.write_text("# comment\n\nQA-Private Name\nzz\n", encoding="utf-8")
        self.image("name.png", png(chunk(b"tEXt", "Author\0qa-private name".encode("latin-1"))))
        with mock.patch.dict(os.environ, {guidance.PRIVATE_STRINGS_ENV: str(strings)}):
            issues = guidance.image_metadata_issues(self.root)
        text = "\n".join(str(issue) for issue in issues)
        self.assertIn("name.png:1: image metadata PNG tEXt holds a configured private string", text)
        self.assertIn(f"{strings}:4: private strings shorter than 3 characters", text)
        self.assertNotIn("Private", text)
        self.assertNotIn("zz", text.replace(str(strings), ""))
        strings.unlink()
        with mock.patch.dict(os.environ, {guidance.PRIVATE_STRINGS_ENV: str(strings)}):
            self.assertIn("cannot read the private strings file", str(guidance.image_metadata_issues(self.root)[0]))

    def test_default_private_strings_file_is_read_from_the_local_directory(self):
        (self.root / guidance.PRIVATE_STRINGS_FILE).parent.mkdir(parents=True)
        (self.root / guidance.PRIVATE_STRINGS_FILE).write_text("QA-Private Name\n", encoding="utf-8")
        self.image("docs/name.jpg", jpeg(segment(0xFE, b"by QA-Private Name")))
        self.assertEqual(self.messages(), ["docs/name.jpg: image metadata JPEG COM holds a configured private "
                                           "string; strip or rewrite that metadata"])

    def test_only_tracked_images_count_in_a_work_tree_and_size_is_bounded(self):
        git = ["git", "-C", str(self.root), "-c", "core.hooksPath=/dev/null"]
        subprocess.run([*git, "init", "-q"], check=True)
        leak = png(chunk(b"tEXt", b"Source\0/home/alice/"))
        self.image("tracked.png", leak)
        self.image("big.jpg", b"\xff\xd8")
        subprocess.run([*git, "add", "tracked.png", "big.jpg"], check=True)
        self.image("untracked.png", leak)
        self.image(".local/private.png", leak)
        with mock.patch.object(guidance, "MAX_IMAGE_BYTES", 96):
            self.image("big.jpg", b"\xff\xd8" + bytes(100))
            self.assertEqual(self.messages(), [
                "big.jpg: image exceeds the 0 MiB metadata scan bound",
                "tracked.png: image metadata PNG tEXt holds an absolute home path; strip or rewrite that metadata",
            ])

    def test_a_decompression_bomb_is_bounded_and_reported(self):
        with mock.patch.object(guidance, "MAX_TEXT_BYTES", 1024):
            self.image("bomb.png", png(chunk(b"zTXt", b"Comment\0\0" + zlib.compress(bytes(4096)))))
            self.assertEqual(self.messages(), ["bomb.png: image metadata PNG zTXt exceeds the 0 MiB text bound "
                                               "or does not decompress"])


if __name__ == "__main__":
    unittest.main()
