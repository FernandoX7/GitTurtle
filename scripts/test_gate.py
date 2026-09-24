"""Unit tests for scripts/gate.py using fakes; no cargo invocation.

Run with: python3 -m unittest scripts/test_gate.py
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gate  # noqa: E402


CRATES = {"crates/app": "gitturtle", "crates/git-core": "gitturtle-core", "crates/preview": "gitturtle-preview"}


class ScopeTests(unittest.TestCase):
    def test_paths_map_to_crates(self):
        scope = gate.classify({"crates/git-core/src/work.rs", "crates/preview/tests/x.rs"}, CRATES)
        self.assertEqual(scope.crates, ("gitturtle-core", "gitturtle-preview"))
        self.assertFalse(scope.widened)
        self.assertTrue(scope.rust_changed)
        self.assertEqual(scope.app_sources, ())

    def test_app_sources_are_tracked(self):
        scope = gate.classify({"crates/app/src/views.rs", "crates/app/docs/x.md"}, CRATES)
        self.assertEqual(scope.crates, ("gitturtle",))
        self.assertEqual(scope.app_sources, ("crates/app/src/views.rs",))

    def test_vendor_and_manifests_widen(self):
        for path in ("vendor/gpui-base/src/lib.rs", "Cargo.lock", "Cargo.toml", ".cargo/config.toml", "tools/x.rs", ".config/nextest.toml"):
            scope = gate.classify({path}, CRATES)
            self.assertTrue(scope.widened, path)
            self.assertTrue(scope.rust_changed, path)

    def test_docs_and_agent_config_do_not_widen(self):
        scope = gate.classify({"AGENTS.md", ".claude/agents/verifier.md", "docs/validation.md", "scripts/gate.py", "website/index.html"}, CRATES)
        self.assertTrue(scope.empty)
        self.assertFalse(scope.rust_changed)

    def test_porcelain_paths_handle_renames_and_untracked(self):
        text = "?? .claude/\n M crates/app/src/main.rs\nR  old.rs -> crates/preview/src/new.rs\n"
        self.assertEqual(
            gate.porcelain_paths(text),
            {".claude/", "crates/app/src/main.rs", "old.rs", "crates/preview/src/new.rs"},
        )

    def test_load_crates_reads_package_names(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/a", "crates/b"]\n')
            (root / "crates/a").mkdir(parents=True)
            (root / "crates/b").mkdir(parents=True)
            (root / "crates/a/Cargo.toml").write_text('[package]\nname = "alpha"\nversion = "0.1.0"\n[dependencies]\nname = "not-this"\n')
            (root / "crates/b/Cargo.toml").write_text('[package]\nname = "beta"\n')
            self.assertEqual(gate.load_crates(root), {"crates/a": "alpha", "crates/b": "beta"})

    def test_gpui_test_names_from_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates/app/src"
            src.mkdir(parents=True)
            (src / "views.rs").write_text(
                "#[gpui::test]\nfn first(cx: &mut TestAppContext) {}\n"
                "#[gpui::test(iterations = 3)]\nasync fn second(cx: &mut TestAppContext) {}\n"
                "#[test]\nfn plain() {}\n"
            )
            self.assertEqual(gate.gpui_test_names(root, ("crates/app/src/views.rs", "missing.rs")), ["first", "second"])


class FilterTests(unittest.TestCase):
    def test_cargo_json_reduces_to_rendered_diagnostics(self):
        messages = []
        for index in range(8):
            messages.append(
                json.dumps(
                    {
                        "reason": "compiler-message",
                        "message": {
                            "level": "error",
                            "message": f"problem {index}",
                            "rendered": f"error: problem {index}\n --> crates/app/src/a.rs:{index + 1}:5\n",
                            "spans": [{"is_primary": True, "file_name": "crates/app/src/a.rs", "line_start": index + 1, "column_start": 5}],
                        },
                    }
                )
            )
        messages.append(json.dumps({"reason": "build-finished", "success": False}))
        messages.append("error: could not compile `gitturtle` (lib) due to 8 previous errors")
        filtered, first = gate.filter_cargo_json(messages)
        self.assertEqual(first, "crates/app/src/a.rs:1:5")
        self.assertIn("... 3 more diagnostics", filtered)
        self.assertEqual(sum(1 for line in filtered if line.startswith("error: problem")), 5)
        self.assertIn("error: could not compile `gitturtle` (lib) due to 8 previous errors", filtered)

    def test_nextest_keeps_failures_and_blocks(self):
        output = [
            "    Starting 3 tests across 2 binaries",
            "        PASS [   0.010s] gitturtle-core tests::ok",
            "        FAIL [   0.030s] gitturtle-core work::authentication::tests::askpass",
            "--- STDOUT:              gitturtle-core work::authentication::tests::askpass ---",
            "thread 'x' panicked at crates/git-core/src/work/authentication.rs:812:9:",
            "assertion failed",
            "--- STDERR:              gitturtle-core work::authentication::tests::askpass ---",
            "could not read Username",
            "        PASS [   0.010s] gitturtle-core::workflow commit_round_trip",
            "     TIMEOUT [  60.000s] gitturtle-core::history paging_stalls",
            "------------",
            "     Summary [   5.123s] 3 tests run: 1 passed, 2 failed, 0 skipped",
            "        FAIL [   0.030s] gitturtle-core work::authentication::tests::askpass",
            "error: test run failed",
        ]
        kept, failed, first = gate.filter_nextest(output)
        self.assertEqual(failed, ("work::authentication::tests::askpass", "paging_stalls"))
        self.assertEqual(first, "crates/git-core/src/work/authentication.rs:812:9")
        self.assertNotIn("        PASS [   0.010s] gitturtle-core tests::ok", kept)
        self.assertIn("could not read Username", kept)

    def test_nextest_0_9_145_format_with_progress_counter(self):
        rule = "─" * 12
        output = [
            rule,
            " Nextest run ID b0bb9fd2 with nextest profile: ci",
            "    Starting 1 test across 22 binaries (290 tests skipped)",
            "        FAIL [   0.014s] (1/1) gitturtle-core work::authentication::tests::askpass",
            "  stdout " + "─" * 3,
            "",
            "    running 1 test",
            "    test work::authentication::tests::askpass ... FAILED",
            "  stderr " + "─" * 3,
            "",
            "    thread 'work::authentication::tests::askpass' (1423692) panicked at crates/git-core/src/work/authentication.rs:627:10:",
            "    called `Result::unwrap()` on an `Err` value: fatal: could not read Username",
            rule,
            "     Summary [   0.015s] 1 test run: 0 passed, 1 failed, 290 skipped",
            "        FAIL [   0.014s] (1/1) gitturtle-core work::authentication::tests::askpass",
            "error: test run failed",
        ]
        kept, failed, first = gate.filter_nextest(output)
        self.assertEqual(failed, ("work::authentication::tests::askpass",))
        self.assertEqual(first, "crates/git-core/src/work/authentication.rs:627:10")
        self.assertIn("    called `Result::unwrap()` on an `Err` value: fatal: could not read Username", kept)
        self.assertNotIn(" Nextest run ID b0bb9fd2 with nextest profile: ci", kept)

    def test_cargo_test_fallback_collects_failed_names(self):
        output = [
            "running 2 tests",
            "test a::b ... ok",
            "test work::authentication::tests::askpass ... FAILED",
            "failures:",
            "---- work::authentication::tests::askpass stdout ----",
            "thread panicked at crates/git-core/src/work/authentication.rs:812:9:",
            "test result: FAILED. 1 passed; 1 failed",
        ]
        kept, failed, first = gate.filter_cargo_test(output)
        self.assertEqual(failed, ("work::authentication::tests::askpass",))
        self.assertEqual(first, "crates/git-core/src/work/authentication.rs:812:9")
        self.assertIn("failures:", kept)

    def test_typos_first_location(self):
        # Build the misspellings at runtime so this file itself stays typo-free.
        first_typo = "te" + "h"
        second_typo = "reci" + "eve"
        kept, first = gate.filter_typos(
            [f"docs/a.md:3:7: `{first_typo}` -> `the`", f"crates/app/src/x.rs:9:1: `{second_typo}` -> `receive`"]
        )
        self.assertEqual(first, "docs/a.md:3:7")
        self.assertEqual(len(kept), 2)

    def test_audit_summary(self):
        report = json.dumps(
            {
                "vulnerabilities": {"list": [{"advisory": {"id": "RUSTSEC-2026-0001", "title": "bad"}, "package": {"name": "dep", "version": "1.0.0"}}]},
                "warnings": {"unmaintained": [{"package": {"name": "old", "version": "0.1.0"}, "advisory": {"id": "RUSTSEC-2025-0002"}}]},
            }
        )
        kept, _ = gate.filter_audit([report])
        self.assertEqual(kept[0], "advisories: 1 vulnerabilities, 1 warnings")
        self.assertIn("RUSTSEC-2026-0001 dep 1.0.0: bad", kept)
        self.assertIn("unmaintained: old 0.1.0 RUSTSEC-2025-0002", kept)

    def test_deny_summary_orders_errors_first(self):
        lines = [
            json.dumps({"type": "diagnostic", "fields": {"severity": "warning", "code": "license-not-encountered", "message": "license was not encountered", "labels": [{"span": "NCSA", "message": "unmatched license allowance"}]}}),
            json.dumps({"type": "diagnostic", "fields": {"severity": "error", "code": "vulnerability", "message": "bad handshake", "graphs": [{"Krate": {"name": "rustls", "version": "0.23.44"}}], "advisory": {"id": "RUSTSEC-2026-0285"}}}),
            json.dumps({"type": "summary", "fields": {"advisories": {"errors": 1}}}),
        ]
        kept, _ = gate.filter_deny(lines)
        self.assertEqual(kept[0], "error[vulnerability] rustls 0.23.44 bad handshake RUSTSEC-2026-0285")
        self.assertEqual(kept[1], "warning[license-not-encountered] license was not encountered NCSA")
        self.assertIn("cargo-deny diagnostics: error=1, summary=1, warning=1", kept)

    def test_controller_tests_stage_sets_umask(self):
        scope = gate.Scope(("gitturtle-core",), True, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("full", scope, Path(tmp), {name: False for name in gate.OPTIONAL_TOOLS}, None, False, CRATES, Path(tmp))
        stage = next(stage for stage in stages if stage.name == "controller-tests")
        self.assertEqual(stage.umask, 0o077)

    def test_plain_first_error_from_panic(self):
        kept, first = gate.filter_plain(["thread 'main' panicked at scripts/x.rs:4:2:", "boom"])
        self.assertEqual(first, "scripts/x.rs:4:2")
        self.assertEqual(kept[-1], "boom")


class KnownFailureTests(unittest.TestCase):
    def outcome(self, kind: str, failed: tuple[str, ...]) -> gate.Outcome:
        stage = gate.Stage("tests:x", ["cargo", "nextest", "run", "-p", "x"], kind=kind)
        return gate.Outcome(stage, 100, 1.0, [], [], None, failed, "failed")

    def test_only_known_failures_pass_with_warn(self):
        outcome = gate.apply_known_failures(self.outcome("nextest", ("a::b",)), {"a::b"})
        self.assertEqual(outcome.status, "warn")
        self.assertIn("a::b", outcome.message)

    def test_unknown_failure_stays_failed(self):
        outcome = gate.apply_known_failures(self.outcome("nextest", ("a::b", "c::d")), {"a::b"})
        self.assertEqual(outcome.status, "failed")

    def test_build_failure_without_names_stays_failed(self):
        outcome = gate.apply_known_failures(self.outcome("nextest", ()), {"a::b"})
        self.assertEqual(outcome.status, "failed")

    def test_non_test_stage_untouched(self):
        outcome = gate.apply_known_failures(self.outcome("cargo-json", ("a::b",)), {"a::b"})
        self.assertEqual(outcome.status, "failed")


class StageTests(unittest.TestCase):
    def tools(self, **present: bool) -> dict[str, bool]:
        return {name: present.get(name, False) for name in gate.OPTIONAL_TOOLS}

    def test_fast_scopes_clippy_and_tests_to_changed_crates(self):
        scope = gate.Scope(("gitturtle-core",), False, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("fast", scope, Path(tmp), self.tools(nextest=True), None, False, CRATES, Path(tmp))
        names = [stage.name for stage in stages]
        self.assertEqual(names, ["format", "typos", "machete", "check", "clippy:gitturtle-core", "tests:gitturtle-core"])
        tests = stages[-1]
        self.assertEqual(tests.kind, "nextest")
        self.assertEqual(tests.argv, ["cargo", "nextest", "run", "--locked", "-p", "gitturtle-core", "-P", "ci", "--no-fail-fast"])
        self.assertIn("--message-format=json", stages[3].argv)

    def test_fast_without_nextest_uses_cargo_test(self):
        scope = gate.Scope(("gitturtle-preview",), False, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("fast", scope, Path(tmp), self.tools(), None, False, CRATES, Path(tmp))
        tests = stages[-1]
        self.assertEqual(tests.kind, "cargo-test")
        self.assertEqual(tests.argv, ["cargo", "test", "--locked", "-p", "gitturtle-preview"])

    def test_widened_scope_runs_workspace(self):
        scope = gate.Scope(("gitturtle",), True, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("fast", scope, Path(tmp), self.tools(nextest=True), None, False, CRATES, Path(tmp))
        names = [stage.name for stage in stages]
        self.assertIn("clippy", names)
        self.assertIn("tests", names)
        self.assertNotIn("clippy:gitturtle", names)

    def test_gpui_iterations_stage_added_for_app_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "crates/app/src"
            src.mkdir(parents=True)
            (src / "views.rs").write_text("#[gpui::test]\nfn renders(cx: &mut TestAppContext) {}\n")
            scope = gate.Scope(("gitturtle",), False, True, ("crates/app/src/views.rs",))
            stages = gate.build_stages("fast", scope, root, self.tools(nextest=True), None, False, CRATES, root)
        stage = stages[-1]
        self.assertEqual(stage.name, "gpui-iterations")
        self.assertEqual(stage.env, {"ITERATIONS": "20"})
        self.assertEqual(stage.argv[-1], "test(/::renders$/)")

    def test_full_adds_candidate_stages_and_mutants_when_diff_exists(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.email", "t@example.com"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "t"], cwd=root, check=True)
            (root / "crates/git-core/src").mkdir(parents=True)
            (root / "crates/git-core/src/lib.rs").write_text("fn a() {}\n")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "base"], cwd=root, check=True)
            base = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=True).stdout.strip()
            (root / "crates/git-core/src/lib.rs").write_text("fn a() { let _ = 1; }\n")
            scope = gate.Scope(("gitturtle-core",), True, True, ())
            stages = gate.build_stages("full", scope, root, self.tools(nextest=True, mutants=True, deny=True, audit=True), base, False, CRATES, root)
        names = [stage.name for stage in stages]
        for expected in ("doctests", "doc", "insta", "deny", "audit", "mutants", "release", "guidance", "controller-tests"):
            self.assertIn(expected, names)
        mutants = next(stage for stage in stages if stage.name == "mutants")
        self.assertTrue(mutants.clear_target_dir)
        self.assertTrue(mutants.advisory)
        self.assertIn("--in-diff", mutants.argv)
        self.assertNotIn("coverage", names)

    def test_coverage_only_with_strict_and_threshold(self):
        scope = gate.Scope(("gitturtle-core",), True, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            os.environ["GITTURTLE_GATE_COVERAGE_MIN"] = "60"
            try:
                stages = gate.build_stages("full", scope, Path(tmp), self.tools(nextest=True, **{"llvm-cov": True}), None, True, CRATES, Path(tmp))
            finally:
                del os.environ["GITTURTLE_GATE_COVERAGE_MIN"]
        coverage = next(stage for stage in stages if stage.name == "coverage")
        self.assertEqual(coverage.argv[-2:], ["--fail-under-lines", "60"])
        self.assertIn("-p", coverage.argv)


class ReportTests(unittest.TestCase):
    def test_render_report_fields_in_order(self):
        stage = gate.Stage("tests:gitturtle-core", ["cargo", "nextest", "run", "-p", "gitturtle-core"], kind="nextest")
        outcome = gate.Outcome(stage, 100, 3.2, ["a", "b"], ["FAIL x", "panicked at crates/git-core/src/a.rs:1:2"], "crates/git-core/src/a.rs:1:2", ("mod::t",), "failed")
        text = gate.render_report("fast", outcome, [outcome], 2, {"nextest": True, "typos": False}, Path("/repo"))
        lines = text.splitlines()
        order = [line.split(":")[0] for line in lines if line.split(":")[0] in ("Stage", "Command", "Exit", "First error", "Next command")]
        self.assertEqual(order, ["Stage", "Command", "Exit", "First error", "Next command"])
        self.assertIn("Next command: cargo nextest run --locked -p gitturtle-core -E 'test(=mod::t)'", lines)
        self.assertIn("Tests removed: 2", lines)
        self.assertIn("Tooling: nextest=yes typos=no", lines)
        self.assertLessEqual(len(lines), gate.MAX_REPORT_LINES)

    def test_removed_tests_counts_diff_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.email", "t@example.com"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "t"], cwd=root, check=True)
            (root / "lib.rs").write_text("#[test]\nfn a() {}\n#[gpui::test]\nfn b() {}\n#[tokio::test]\nasync fn c() {}\n")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "base"], cwd=root, check=True)
            base = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=True).stdout.strip()
            (root / "lib.rs").write_text("fn a() {}\n#[gpui::test]\nfn b() {}\n")
            self.assertEqual(gate.removed_tests(root, base), 2)
            self.assertEqual(gate.removed_tests(root, None), 0)


class CliTests(unittest.TestCase):
    def test_usage_error_exit_code(self):
        self.assertEqual(gate.main(["nightly"]), 4)
        self.assertEqual(gate.main(["fast", "--changed-only", "--workspace"]), 4)

    def test_missing_known_failures_file_is_usage_error(self):
        self.assertEqual(gate.main(["fast", "--known-failures", "/nonexistent/known.txt"]), 4)

    def test_shell_quote(self):
        self.assertEqual(gate.shell_quote("cargo"), "cargo")
        self.assertEqual(gate.shell_quote("test(/x$/)"), "'test(/x$/)'")


if __name__ == "__main__":
    unittest.main()
