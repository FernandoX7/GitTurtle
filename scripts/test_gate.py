"""Unit tests for scripts/gate.py using fakes; no cargo invocation.

Run with: python3 -m unittest scripts/test_gate.py
"""

from __future__ import annotations

import importlib.util
import itertools
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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
                "#[gpui_kit::test]\nasync fn third(cx: &mut gpui_kit::TestAppContext) {}\n"
                "#[test]\nfn plain() {}\n"
                "#[other_kit::test]\nfn foreign() {}\n"
            )
            self.assertEqual(gate.gpui_test_names(root, ("crates/app/src/views.rs", "missing.rs")), ["first", "second", "third"])


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


class GitIsolationTests(unittest.TestCase):
    ALL_TOOLS = {name: True for name in gate.OPTIONAL_TOOLS}

    def full_stages(self, root: Path) -> list[gate.Stage]:
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "base"], check=True)
        base = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
        (root / "crates/app/src").mkdir(parents=True)
        (root / "crates/app/src/views.rs").write_text("#[gpui::test]\nfn draws() {}\n")
        subprocess.run(["git", "-C", str(root), "add", "."], check=True)
        scope = gate.Scope(("gitturtle",), True, True, ("crates/app/src/views.rs",))
        with patch.dict(os.environ, {"GITTURTLE_GATE_COVERAGE_MIN": "50"}):
            return gate.build_stages("full", scope, root, self.ALL_TOOLS, base, True, CRATES, root)

    def test_every_test_stage_is_isolated_and_nothing_else(self):
        with tempfile.TemporaryDirectory() as tmp:
            stages = self.full_stages(Path(tmp))
            isolated = {stage.name for stage in stages if stage.unset_env}
            self.assertEqual(isolated, {"tests", "gpui-iterations", "doctests", "mutants", "coverage"})
            for stage in stages:
                if stage.name not in isolated:
                    self.assertNotIn("GIT_CONFIG_GLOBAL", stage.env, stage.name)
                    continue
                self.assertEqual(stage.env["GIT_CONFIG_NOSYSTEM"], "1")
                global_config = Path(stage.env["GIT_CONFIG_GLOBAL"])
                self.assertEqual(global_config.parent, Path(tmp))
                self.assertEqual(global_config.read_text(), "")
                self.assertTrue({"GIT_ASKPASS", "SSH_ASKPASS"} <= set(stage.unset_env))
                self.assertTrue(stage.reproduce.startswith("env -u GIT_ASKPASS -u SSH_ASKPASS "), stage.reproduce)

    def test_fast_crate_test_stages_are_isolated(self):
        scope = gate.Scope(("gitturtle-core", "gitturtle-preview"), False, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("fast", scope, Path(tmp), {"nextest": True}, None, False, CRATES, Path(tmp))
        tests = [stage for stage in stages if stage.name.startswith("tests:")]
        self.assertEqual(len(tests), 2)
        self.assertTrue(all(stage.unset_env == gate.GIT_ISOLATION_UNSET for stage in tests))
        self.assertTrue(all(not stage.unset_env for stage in stages if not stage.name.startswith("tests:")))

    def test_run_stage_drops_inherited_askpass_and_configuration(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "env.json"
            stage = gate.Stage("tests", [sys.executable, "-c", "import json, os, sys; json.dump(dict(os.environ), open(sys.argv[1], 'w'))", str(dump)])
            [stage] = gate.isolate_test_stages([stage], root)
            hostile = {
                "GIT_ASKPASS": "/usr/bin/false",
                "SSH_ASKPASS": "/usr/bin/false",
                "SSH_ASKPASS_REQUIRE": "force",
                "GIT_CONFIG_PARAMETERS": "'credential.username'='hostile'",
                "GIT_CONFIG_COUNT": "1",
                "GIT_CONFIG_NOSYSTEM": "0",
                "GIT_CONFIG_GLOBAL": "/hostile/gitconfig",
            }
            with patch.dict(os.environ, hostile):
                outcome = gate.run_stage(stage, root, "fast")
            self.assertEqual(outcome.returncode, 0, outcome.lines)
            env = json.loads(dump.read_text())
        for name in gate.GIT_ISOLATION_UNSET:
            self.assertNotIn(name, env)
        self.assertEqual(env["GIT_CONFIG_NOSYSTEM"], "1")
        self.assertEqual(env["GIT_CONFIG_GLOBAL"], str(root / "gitconfig"))

    def test_next_command_for_an_isolated_test_keeps_the_isolation(self):
        stage = gate.Stage("tests:gitturtle-core", ["cargo", "nextest", "run", "--locked", "-p", "gitturtle-core"], kind="nextest")
        with tempfile.TemporaryDirectory() as tmp:
            [stage] = gate.isolate_test_stages([stage], Path(tmp))
        outcome = gate.Outcome(stage, 100, 1.0, [], [], None, ("a::b",), "failed")
        command = gate.narrow_command(outcome)
        self.assertTrue(command.startswith(gate.GIT_ISOLATION_PREFIX), command)
        self.assertTrue(command.endswith("cargo nextest run --locked -p gitturtle-core -E 'test(=a::b)'"), command)


class StageTests(unittest.TestCase):
    def tools(self, **present: bool) -> dict[str, bool]:
        return {name: present.get(name, False) for name in gate.OPTIONAL_TOOLS}

    def test_fast_type_checks_once_and_scopes_tests_to_changed_crates(self):
        scope = gate.Scope(("gitturtle-core",), False, True, ())
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("fast", scope, Path(tmp), self.tools(nextest=True), None, False, CRATES, Path(tmp))
        names = [stage.name for stage in stages]
        # Workspace clippy covers every target the old workspace check built.
        self.assertEqual(names, ["format", "typos", "machete", "clippy", "tests:gitturtle-core"])
        self.assertEqual(stages[3].reproduce, "cargo clippy --locked --workspace --all-targets -- -D warnings")
        self.assertIn("--message-format=json", stages[3].argv)
        tests = stages[-1]
        self.assertEqual(tests.kind, "nextest")
        self.assertEqual(tests.argv, ["cargo", "nextest", "run", "--locked", "-p", "gitturtle-core", "-P", "ci", "--no-fail-fast"])

    def test_fast_without_rust_changes_keeps_the_workspace_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("fast", gate.Scope((), False, False, ()), Path(tmp), self.tools(), None, False, CRATES, Path(tmp))
        self.assertEqual([stage.name for stage in stages], ["format", "typos", "machete", "check"])
        self.assertEqual(stages[-1].reproduce, "cargo check --locked --workspace --all-targets")

    def test_stages_without_a_rust_build_run_first(self):
        build_free = {"format", "typos", "machete", "insta", "privacy", "deny", "audit", "guidance", "controller-tests"}
        scopes = (
            (gate.Scope(("gitturtle-core",), False, True, ()), "clippy"),
            (gate.Scope(tuple(sorted(CRATES.values())), True, True, ()), "clippy"),
            (gate.Scope((), False, False, ()), "check"),
        )
        for tier in ("fast", "full"):
            for scope, first_build in scopes:
                with tempfile.TemporaryDirectory() as tmp:
                    tools = {name: True for name in gate.OPTIONAL_TOOLS}
                    stages = gate.build_stages(tier, scope, Path(tmp), tools, None, False, CRATES, Path(tmp))
                names = [stage.name for stage in stages]
                leading = len(list(itertools.takewhile(build_free.__contains__, names)))
                self.assertFalse(build_free & set(names[leading:]), f"{tier}: {names}")
                self.assertEqual(names[leading], first_build, f"{tier}: {names}")
                self.assertEqual(names.count("check") + names.count("clippy"), 1, f"{tier}: {names}")

    def test_deny_never_rewrites_the_lockfile(self):
        with tempfile.TemporaryDirectory() as tmp:
            stages = gate.build_stages("full", gate.Scope((), False, False, ()), Path(tmp), self.tools(deny=True), None, False, CRATES, Path(tmp))
        deny = next(stage for stage in stages if stage.name == "deny")
        self.assertEqual(deny.argv[:3], ["cargo", "deny", "--locked"])

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
        self.assertNotIn("tests:gitturtle", names)

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
        self.assertEqual(stage.env["ITERATIONS"], "20")
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


HAVE_PIL = importlib.util.find_spec("PIL") is not None


class PrivacyStageTests(unittest.TestCase):
    """The image privacy stage, with synthetic templates rendered from a harmless string."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve() / "repo"
        self.root.mkdir()
        environment = patch.dict(os.environ, {"XDG_CACHE_HOME": str(Path(self.tmp.name) / "cache")})
        environment.start()
        self.addCleanup(environment.stop)
        os.environ.pop(gate.PRIVACY_TEMPLATES_ENV, None)
        self.git("init", "-q", "-b", "main")
        (self.root / "docs").mkdir()
        (self.root / "docs/old.png").write_bytes(b"old")
        (self.root / "docs/gone.png").write_bytes(b"gone")
        self.git("add", ".")
        self.git("-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "base")
        self.base = self.git("rev-parse", "HEAD")

    def git(self, *args: str) -> str:
        return subprocess.run(["git", "-C", str(self.root), *args], capture_output=True, text=True, check=True).stdout.strip()

    def stage(self) -> gate.Stage:
        return gate.privacy_stage(self.root, self.base)

    def test_without_templates_the_stage_is_skipped_and_never_passes(self) -> None:
        stage = self.stage()
        self.assertIsNone(stage.argv)
        self.assertIsNone(stage.run)
        self.assertIn("NOT scanned", stage.note)
        scope = gate.Scope(("gitturtle-core",), True, True, ())
        stages = gate.build_stages("full", scope, self.root, {name: False for name in gate.OPTIONAL_TOOLS}, self.base, False, CRATES, Path(self.tmp.name))
        self.assertIn("privacy", [stage.name for stage in stages])
        fast = gate.build_stages("fast", scope, self.root, {name: False for name in gate.OPTIONAL_TOOLS}, self.base, False, CRATES, Path(self.tmp.name))
        self.assertNotIn("privacy", [stage.name for stage in fast])

    def test_changed_images_exclude_deletions_other_files_and_symlinks(self) -> None:
        (self.root / "docs/gone.png").unlink()
        (self.root / "docs/old.png").write_bytes(b"changed")
        (self.root / "docs/new.JPG").write_bytes(b"new")
        (self.root / "docs/notes.md").write_text("text")
        (self.root / "docs/link.png").symlink_to("old.png")
        self.assertEqual(gate.changed_images(self.root, self.base), (["docs/new.JPG", "docs/old.png"], 0))

    def test_non_ascii_names_are_scanned_and_unprintable_ones_refused(self) -> None:
        (self.root / "docs/ñandú frame.png").write_bytes(b"new")
        (self.root / "docs/sub").mkdir()
        (self.root / "docs/sub/café.webp").write_bytes(b"new")
        self.git("add", "docs/ñandú frame.png")
        self.assertEqual(gate.changed_images(self.root, self.base), (["docs/sub/café.webp", "docs/ñandú frame.png"], 0))
        (self.root / "docs/line\nbreak.png").write_bytes(b"new")
        refused = 1
        try:
            (self.root / os.fsdecode(b"docs/latin-\xe9.png")).write_bytes(b"new")
            refused += 1
        except OSError:
            pass  # file systems such as APFS refuse names that are not UTF-8
        self.assertEqual(gate.changed_images(self.root, self.base)[1], refused)
        (self.root / gate.PRIVACY_TEMPLATES_DIR).mkdir(parents=True)
        stage = self.stage()
        self.assertEqual(stage.run(), (1, f"{refused} added or changed image name(s) are not printable UTF-8; "
                                          "rename them so they can be scanned\n"))

    def test_default_directory_in_the_checkout_and_a_missing_configured_directory(self) -> None:
        (self.root / gate.PRIVACY_TEMPLATES_DIR).mkdir(parents=True)
        self.assertEqual(gate.privacy_templates(self.root), self.root / gate.PRIVACY_TEMPLATES_DIR)
        (self.root / "docs/old.png").write_bytes(b"changed")
        with patch.dict(os.environ, {gate.PRIVACY_TEMPLATES_ENV: str(Path(self.tmp.name) / "absent")}):
            stage = self.stage()
            code, text = stage.run()
        self.assertEqual(code, 2)
        self.assertNotIn(self.tmp.name, text + stage.reproduce)

    def test_no_changed_images_is_reported_as_such(self) -> None:
        (self.root / gate.PRIVACY_TEMPLATES_DIR).mkdir(parents=True)
        stage = self.stage()
        self.assertEqual(stage.run(), (0, "no added or changed images\n"))

    def test_failed_image_listing_fails_instead_of_reporting_no_images(self) -> None:
        (self.root / gate.PRIVACY_TEMPLATES_DIR).mkdir(parents=True)
        self.assertIsNone(gate.changed_images(self.root, "0" * 40))  # a base Git cannot resolve
        with patch.object(gate, "git_paths", return_value=None):
            code, text = self.stage().run()
        self.assertEqual(code, 1)
        self.assertIn("nothing was scanned", text)

    @unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
    def test_scan_names_only_images_and_verdicts(self) -> None:
        from PIL import Image, ImageDraw, ImageFont

        font = ImageFont.load_default()
        text = Image.new("L", (80, 14), 255)
        ImageDraw.Draw(text).text((2, 1), "QA-TEMPLATE", font=font, fill=0)
        templates = Path(self.tmp.name) / "templates"
        templates.mkdir()
        text.save(templates / "QA-TEMPLATE-secret-name.png")
        leaky = Image.new("L", (160, 60), 230)
        leaky.paste(text, (40, 20))
        leaky.convert("RGB").save(self.root / "docs/old.png")
        Image.new("RGB", (160, 60), (230, 230, 230)).save(self.root / "docs/clean.png")
        with patch.dict(os.environ, {gate.PRIVACY_TEMPLATES_ENV: str(templates)}):
            stage = self.stage()
            code, output = stage.run()
        self.assertEqual(code, 1, output)
        self.assertEqual(output.splitlines(), ["docs/clean.png: clean", "docs/old.png: MATCH",
                                               "privacy scan: 2 image(s), 1 matched a template"])
        outcome = gate.Outcome(stage, code, 1.0, output.splitlines(), output.splitlines(), None, (), "failed")
        report = gate.render_report("full", outcome, [outcome], 0, {}, self.root)
        for secret in ("QA-TEMPLATE", str(templates)):
            self.assertNotIn(secret, output + stage.reproduce + stage.note + report)


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

    def test_known_failures_allowlist_is_retired(self):
        self.assertEqual(gate.main(["fast", "--known-failures", "/nonexistent/known.txt"]), 4)
        self.assertFalse(hasattr(gate, "apply_known_failures"))

    def test_shell_quote(self):
        self.assertEqual(gate.shell_quote("cargo"), "cargo")
        self.assertEqual(gate.shell_quote("test(/x$/)"), "'test(/x$/)'")


if __name__ == "__main__":
    unittest.main()
