"""Unit tests for scripts/check-docs-sync.py using isolated fixtures."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

_SCRIPT = Path(__file__).resolve().parents[1] / "check-docs-sync.py"
_SPEC = importlib.util.spec_from_file_location("check_docs_sync", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
docs_sync = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = docs_sync
_SPEC.loader.exec_module(docs_sync)


class DocsSyncTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.write("rust-toolchain.toml", '[toolchain]\nchannel = "1.100.0"\n')
        self.write("Cargo.lock", '[[package]]\nname = "ufbx"\nversion = "0.11.5"\n')
        self.write(".github/workflows/quality.yml", "name: Quality\n")
        self.write(".github/workflows/release.yml", "name: Release\n")
        self.write("crates/app/src/appearance.rs", "pub const ALL: [Self; 20] = [];\n")
        self.write("crates/app/src/prefs.rs", "pub const MAX_CUSTOM: usize = 32;\nconst MAX_BYTES: usize = 64 * 1024 * 1024;\n")
        self.write("crates/app/src/included.txt", "ok\n")
        self.write("crates/app/src/lib.rs", 'const DATA: &str = include_str!("included.txt");\n')
        self.write("README.md", "Selects Rust **1.100.0**. Twenty built-in themes and up to 32 custom themes, 64 MiB max.\n")
        self.write("CONTRIBUTING.md", "Workflows: `quality.yml`, `release.yml`. Uses Rust 1.100.0.\n")
        self.write("docs/previews.md", "See https://docs.rs/ufbx/0.11.5/ufbx/ for details.\n")
        self.write(
            "docs/doc-manifest.json",
            json.dumps(
                {
                    "version": 1,
                    "rust_toolchain_docs": ["README.md", "CONTRIBUTING.md"],
                    "lockfile_crate_mentions": [
                        {
                            "crate": "ufbx",
                            "doc": "docs/previews.md",
                            "pattern": r"docs\.rs/ufbx/([0-9]+\.[0-9]+\.[0-9]+)",
                        }
                    ],
                    "constants": [
                        {
                            "name": "built_in_themes",
                            "source": "crates/app/src/appearance.rs",
                            "kind": "regex_int",
                            "pattern": r"pub const ALL:\s*\[Self;\s*(\d+)\]",
                            "docs": [
                                {"path": "README.md", "pattern": r"\b{word_value} built-in themes\b", "ignore_case": True}
                            ],
                        },
                        {
                            "name": "max_custom",
                            "source": "crates/app/src/prefs.rs",
                            "kind": "rust_const_int",
                            "symbol": "MAX_CUSTOM",
                            "docs": [{"path": "README.md", "pattern": r"\b{value} custom themes\b"}],
                        },
                        {
                            "name": "max_mib",
                            "source": "crates/app/src/prefs.rs",
                            "kind": "rust_const_mib",
                            "symbol": "MAX_BYTES",
                            "docs": [{"path": "README.md", "pattern": r"\b{value} MiB\b"}],
                        },
                    ],
                    "workflows_listed_in": ["CONTRIBUTING.md"],
                    "include_str_roots": ["crates"],
                    "check_website_guides": False,
                    "surface_map": [
                        {"code_prefixes": ["crates/app/src/appearance.rs"], "docs": ["README.md"]},
                    ],
                },
                indent=2,
            ),
        )

    def write(self, rel: str, text: str) -> Path:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def messages(self) -> str:
        return "\n".join(str(issue) for issue in docs_sync.validate(self.root))

    def test_valid_fixture_passes(self):
        self.assertEqual(docs_sync.validate(self.root), [])

    def test_stale_rust_version_fails(self):
        self.write("README.md", "Selects Rust **1.99.0**. Twenty built-in themes and up to 32 custom themes, 64 MiB max.\n")
        msg = self.messages()
        self.assertIn("README.md: stale Rust version(s) 1.99.0 (rust-toolchain.toml pins 1.100.0)", msg)

    def test_stale_lockfile_crate_version_fails(self):
        self.write("docs/previews.md", "See https://docs.rs/ufbx/0.10.0/ufbx/ for details.\n")
        msg = self.messages()
        self.assertIn("docs/previews.md: stale ufbx version(s) 0.10.0 (Cargo.lock has 0.11.5)", msg)

    def test_drifted_constants_fail(self):
        self.write("crates/app/src/prefs.rs", "pub const MAX_CUSTOM: usize = 40;\nconst MAX_BYTES: usize = 128 * 1024 * 1024;\n")
        msg = self.messages()
        self.assertIn("constant max_custom=40", msg)
        self.assertIn("constant max_mib=128", msg)

    def test_missing_workflow_mention_fails(self):
        self.write(".github/workflows/nightly.yml", "name: Nightly\n")
        self.assertIn("workflow 'nightly.yml' exists in .github/workflows/ but is not mentioned", self.messages())

    def test_missing_and_escaping_include_str_fail(self):
        self.write(
            "crates/app/src/lib.rs",
            'const A: &str = include_str!("absent.txt");\nconst B: &str = include_str!("../../../../etc/hosts");\n',
        )
        msg = self.messages()
        self.assertIn("include_str! target missing: 'absent.txt'", msg)
        self.assertIn("include_str! escapes repo:", msg)

    def test_website_guides_check_failure_is_reported(self):
        manifest = json.loads((self.root / "docs/doc-manifest.json").read_text(encoding="utf-8"))
        manifest["check_website_guides"] = True
        self.write("docs/doc-manifest.json", json.dumps(manifest))
        self.write("website/build_guides.py", "import sys\nprint('stale guide', file=sys.stderr)\nsys.exit(1)\n")
        self.assertIn("generated website guides/notices out of date: stale guide", self.messages())

    def test_changed_since_reports_affected_surfaces(self):
        git = ["git", "-C", str(self.root), "-c", "core.hooksPath=/dev/null"]
        subprocess.run([*git, "init", "-q", "-b", "main"], check=True)
        subprocess.run([*git, "add", "."], check=True)
        subprocess.run([*git, "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "init"], check=True)
        self.write("crates/app/src/appearance.rs", "pub const ALL: [Self; 20] = [/* edit */];\n")
        surfaces = docs_sync.affected_surfaces(self.root, "HEAD")
        self.assertEqual(surfaces, [("crates/app/src/appearance.rs", ["README.md"])])


if __name__ == "__main__":
    unittest.main()
