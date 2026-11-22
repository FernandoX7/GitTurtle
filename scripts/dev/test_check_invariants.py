"""Tests for the build-free architectural fitness linter."""
from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev import check_invariants  # noqa: E402


def _make_clean_repo(root: Path) -> None:
    (root / "crates/git-core/src").mkdir(parents=True)
    (root / "crates/preview/src").mkdir(parents=True)
    (root / "crates/app/src").mkdir(parents=True)
    (root / "crates/git-core/Cargo.toml").write_text(
        '[package]\nname = "gitturtle-core"\n[dependencies]\nanyhow = "1"\n',
        encoding="utf-8",
    )
    (root / "crates/git-core/src/lib.rs").write_text(
        "#![forbid(unsafe_code)]\n"
        "#![cfg_attr(not(test), deny(clippy::unwrap_used))]\n"
        'fn git_command() { let _ = std::process::Command::new("git"); }\n',
        encoding="utf-8",
    )
    (root / "crates/preview/Cargo.toml").write_text(
        '[package]\nname = "gitturtle-preview"\n[dependencies]\nanyhow = "1"\n',
        encoding="utf-8",
    )
    (root / "crates/preview/src/lib.rs").write_text(
        "#![cfg_attr(not(test), deny(clippy::unwrap_used))]\n"
        "pub fn decode(bytes: &[u8]) -> usize { bytes.len() }\n"
        "#[cfg(test)]\nmod tests {\n    fn fixture() { let _ = std::fs::read(\"a\"); }\n}\n",
        encoding="utf-8",
    )
    (root / "crates/app/src/main.rs").write_text(
        "struct View;\nimpl View { fn render(&mut self) -> i32 { 0 } }\n",
        encoding="utf-8",
    )


class CheckInvariantsTests(unittest.TestCase):
    def test_clean_repo_and_live_workspace_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            self.assertEqual(check_invariants.check_repo(repo), [])
        self.assertEqual(check_invariants.check_repo(ROOT), [])

    def test_flags_git_command_outside_centralized_builders(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            (repo / "crates/app/src/sneaky.rs").write_text(
                'fn run() { let _ = std::process::Command::new("git"); }\n',
                encoding="utf-8",
            )
            violations = check_invariants.check_repo(repo)
            self.assertEqual([v.rule for v in violations], ["git-command-boundary"])
            self.assertEqual(violations[0].path, "crates/app/src/sneaky.rs")

    def test_ignores_cfg_test_blocks_and_external_test_modules(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            (repo / "crates/app/src/main.rs").write_text(
                "#[cfg(\n    all(\n        test,\n        unix\n    )\n)]\n#[path = \"helper_bench.rs\"]\nmod benchmarks;\n"
                "#[cfg(all(test, unix))]\nmod inline_tests {\n"
                '    fn t() { let _ = std::process::Command::new("git"); }\n'
                "}\n",
                encoding="utf-8",
            )
            (repo / "crates/app/src/helper_bench.rs").write_text(
                'pub fn setup() { let _ = std::process::Command::new("git"); }\n',
                encoding="utf-8",
            )
            self.assertEqual(check_invariants.check_repo(repo), [])

    def test_flags_preview_fs_net_gpui_and_cargo_deps(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            (repo / "crates/preview/src/bad.rs").write_text(
                "use std::net::TcpStream;\n"
                "pub fn bad() { let _ = std::fs::read(\"x\"); let _ = gpui::div(); }\n",
                encoding="utf-8",
            )
            (repo / "crates/preview/Cargo.toml").write_text(
                '[package]\nname = "gitturtle-preview"\n[dependencies]\ngpui-kit = "0.10"\n',
                encoding="utf-8",
            )
            rules = [v.rule for v in check_invariants.check_repo(repo)]
            self.assertIn("crate-boundary", rules)
            self.assertGreaterEqual(rules.count("preview-purity"), 3)

    def test_flags_blocking_io_inside_render(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            (repo / "crates/app/src/view.rs").write_text(
                "impl View {\n"
                "    fn render(&mut self, _window: &mut Window) -> i32 {\n"
                '        let _ = std::fs::write("out.txt", b"hi");\n'
                "        0\n"
                "    }\n"
                "}\n",
                encoding="utf-8",
            )
            violations = check_invariants.check_repo(repo)
            self.assertEqual([v.rule for v in violations], ["ui-render-nonblocking"])
            self.assertEqual(violations[0].line, 3)

    def test_flags_missing_git_core_safety_attributes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            (repo / "crates/git-core/src/lib.rs").write_text("pub fn ok() {}\n", encoding="utf-8")
            violations = check_invariants.check_repo(repo)
            self.assertEqual(len(violations), 2)
            self.assertTrue(all(v.rule == "crate-safety" for v in violations))

    def test_cfg_boolean_parser_never_skips_production_or_nested_not(self) -> None:
        self.assertTrue(check_invariants._is_test_cfg("test"))
        self.assertTrue(check_invariants._is_test_cfg('all(test, target_os = "linux")'))
        self.assertTrue(check_invariants._is_test_cfg('any(test, feature = "test-support")'))
        self.assertFalse(check_invariants._is_test_cfg('any(target_os = "linux", test)'))
        self.assertFalse(
            check_invariants._is_test_cfg('any(target_os = "linux", test, feature = "test-support")')
        )
        self.assertFalse(check_invariants._is_test_cfg("not(test)"))
        self.assertFalse(
            check_invariants._is_test_cfg('not(all(not(target_os = "windows"), test))')
        )

    def test_flags_production_test_support_feature_and_headless_crate_boundaries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            _make_clean_repo(repo)
            (repo / "crates/diff-model/src").mkdir(parents=True)
            (repo / "crates/diff-model/src/lib.rs").write_text(
                "#![forbid(unsafe_code)]\n#![cfg_attr(not(test), deny(clippy::unwrap_used))]\n",
                encoding="utf-8",
            )
            (repo / "crates/diff-model/Cargo.toml").write_text(
                '[package]\nname = "gitturtle-diff-model"\n[dependencies]\ngitturtle-theme = { path = "../theme" }\n',
                encoding="utf-8",
            )
            (repo / "crates/app/Cargo.toml").write_text(
                '[package]\nname = "gitturtle"\n[dependencies]\n'
                'gitturtle-github = { path = "../github", features = ["test-support"] }\n',
                encoding="utf-8",
            )
            rules = [v.rule for v in check_invariants.check_repo(repo)]
            self.assertIn("crate-boundary", rules)
            self.assertIn("test-support-isolation", rules)


if __name__ == "__main__":
    unittest.main()
