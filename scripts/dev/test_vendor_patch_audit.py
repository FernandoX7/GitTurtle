"""Tests for scripts/dev/vendor_patch_audit.py."""
from __future__ import annotations

import hashlib
import io
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev import vendor_patch_audit  # noqa: E402


def _make_crate_archive(dest: Path, name: str, version: str, files: dict[str, bytes]) -> str:
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(dest, mode="w:gz") as tf:
        for rel, content in sorted(files.items()):
            info = tarfile.TarInfo(name=f"{name}-{version}/{rel}")
            info.size = len(content)
            tf.addfile(info, io.BytesIO(content))
    return hashlib.sha256(dest.read_bytes()).hexdigest()


def _make_clean_vendor_repo(root: Path, *, sha256: str = "a" * 64, version: str = "1.2.3") -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "Cargo.toml").write_text(
        '[workspace]\n'
        'members = []\n'
        'exclude = ["vendor/demo-crate"]\n'
        '[patch.crates-io]\n'
        'demo-crate = { path = "vendor/demo-crate" }\n',
        encoding="utf-8",
    )
    crate = root / "vendor" / "demo-crate"
    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text(
        f'[package]\nname = "demo-crate"\nversion = "{version}"\n',
        encoding="utf-8",
    )
    (crate / "src" / "lib.rs").write_text("pub fn patched() -> u32 { 2 }\n", encoding="utf-8")
    (crate / "LICENSE-APACHE").write_text("Apache-2.0\n", encoding="utf-8")
    (crate / "GITTURTLE-PATCH.md").write_text(
        f"# Patch to demo-crate {version}\n\nArchive SHA-256: `{sha256}`.\n\n## Removal\n\nDrop when upstream.\n",
        encoding="utf-8",
    )
    (root / "vendor" / "AGENTS.md").write_text(
        "| Dependency | Note |\n| --- | --- |\n| `demo-crate` | [GITTURTLE-PATCH.md](demo-crate/GITTURTLE-PATCH.md) |\n",
        encoding="utf-8",
    )
    (root / "THIRD_PARTY_NOTICES.md").write_text(
        "| Vendored package | License | Modification record |\n"
        "| --- | --- | --- |\n"
        f"| `demo-crate` {version} | [Apache-2.0](vendor/demo-crate/LICENSE-APACHE) | [Patch](vendor/demo-crate/GITTURTLE-PATCH.md) |\n"
        "\n## Other table\n\n"
        "| Asset | License |\n| --- | --- |\n| `not-a-crate` 9.9.9 | MIT |\n",
        encoding="utf-8",
    )


class VendorPatchAuditTests(unittest.TestCase):
    def test_clean_fixture_and_live_repo_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_vendor_repo(repo)
            violations, summaries = vendor_patch_audit.audit_repo(repo)
            self.assertEqual(violations, [])
            self.assertEqual(len(summaries), 1)
            self.assertEqual(summaries[0]["version"], "1.2.3")

        live_violations, live_summaries = vendor_patch_audit.audit_repo(ROOT)
        self.assertEqual(live_violations, [], [v.format() for v in live_violations])
        self.assertGreaterEqual(len(live_summaries), 1)

    def test_flags_substring_version_symlinks_duplicate_notices_and_missing_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_vendor_repo(repo, version="0.3.1")
            # 0.3.12 must NOT satisfy version 0.3.1.
            (repo / "vendor/demo-crate/GITTURTLE-PATCH.md").write_text(
                f"# Patch 0.3.12\n\nSHA: {'a' * 64}\n\n## Removal\n\nDrop.\n",
                encoding="utf-8",
            )
            (repo / "THIRD_PARTY_NOTICES.md").write_text(
                "| Vendored package | License | Modification record |\n"
                "| --- | --- | --- |\n"
                "| `demo-crate` 0.3.1 | MIT | Patch |\n"
                "| `demo-crate` 0.3.2 | MIT | Patch |\n",
                encoding="utf-8",
            )
            (repo / "vendor/demo-crate/README.gitturtle.md").symlink_to("dangling-target")
            (repo / "vendor/demo-crate/LICENSE-APACHE").write_text("", encoding="utf-8")
            (repo / "vendor/symlinked-crate").symlink_to(repo / "vendor/demo-crate")

            violations, _ = vendor_patch_audit.audit_repo(repo)
            rules = {v.rule for v in violations}
            self.assertIn("patch-note-version", rules)
            self.assertIn("third-party-notices", rules)
            self.assertIn("patch-note-name", rules)
            self.assertIn("license", rules)
            self.assertIn("vendor-symlink", rules)

    def test_flags_cargo_toml_and_vendor_agents_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_vendor_repo(repo)
            (repo / "Cargo.toml").write_text(
                '[workspace]\nmembers = []\nexclude = []\n[patch.crates-io]\n',
                encoding="utf-8",
            )
            (repo / "vendor/AGENTS.md").write_text("# Empty\n", encoding="utf-8")
            violations, _ = vendor_patch_audit.audit_repo(repo)
            rules = {v.rule for v in violations}
            self.assertIn("patch-table", rules)
            self.assertIn("workspace-exclude", rules)
            self.assertIn("vendor-agents", rules)

    def test_diff_cache_reports_deltas_and_verifies_archive_sha256(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            repo = root / "repo"
            cache = root / "cache" / "index.crates.io-test"
            archive = cache / "demo-crate-1.2.3.crate"
            sha = _make_crate_archive(
                archive,
                "demo-crate",
                "1.2.3",
                {
                    "Cargo.toml": b'[package]\nname = "demo-crate"\nversion = "1.2.3"\n',
                    "src/lib.rs": b"pub fn patched() -> u32 { 1 }\n",
                    "LICENSE-APACHE": b"Apache-2.0\n",
                },
            )
            _make_clean_vendor_repo(repo, sha256=sha)
            violations, summaries = vendor_patch_audit.audit_repo(
                repo, diff_cache=True, cargo_cache_roots=[root / "cache"]
            )
            self.assertEqual(violations, [])
            stats = summaries[0]["cache"]
            self.assertEqual(stats["changed_files"], 1)
            self.assertEqual(stats["lines_added"], 1)
            self.assertEqual(stats["lines_removed"], 1)

            # Wrong checksum fails when cache archive is present.
            (repo / "vendor/demo-crate/GITTURTLE-PATCH.md").write_text(
                f"# Patch 1.2.3\n\nSHA: {'b' * 64}\n\n## Removal\n\nLater.\n",
                encoding="utf-8",
            )
            bad_violations, _ = vendor_patch_audit.audit_repo(
                repo, diff_cache=True, cargo_cache_roots=[root / "cache"]
            )
            self.assertIn("archive-checksum-mismatch", {v.rule for v in bad_violations})


if __name__ == "__main__":
    unittest.main()
