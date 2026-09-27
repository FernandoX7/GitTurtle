"""Executed-test extraction from retained libtest and nextest log formats; no network required."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ci_executed_tests", ROOT / "scripts/ci/executed_tests.py")
executed_tests = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(executed_tests)

# Shapes copied from Quality run 36164627195's raw job log (`[ci]` wrapper,
# timestamps, a nested re-executed test binary) and `gh run view --log` output.
CARGO_TEST = """\
2026-09-25T17:07:36.5032394Z [ci]      Running unittests src/main.rs (target/debug/deps/gitturtle-226004413cd0dbc4)
2026-09-25T17:07:36.5105898Z [ci] running 3 tests
2026-09-25T17:07:36.5161571Z [ci] test activity::tests::retains_nothing ... ok
2026-09-25T17:07:37.9288611Z [ci] test github::transport::tests::installed_cli ... ignored, requires installed gh
2026-09-25T17:07:38.0000000Z [ci] test panics::tests::rejects - should panic ... \x1b[32mok\x1b[0m
2026-09-25T17:07:38.1000000Z Running kernel seems to be up-to-date.
2026-09-25T17:08:08.8108908Z [ci]      Running unittests src/lib.rs (target/debug/deps/gitturtle_core-ec9e15583c81313a)
2026-09-25T17:08:08.9000000Z [ci] test history::tests::pages ... ok
2026-09-25T17:08:09.4355678Z [ci]      Running tests/worktrees_reflog.rs (target/debug/deps/worktrees_reflog-163d73fe93f63508)
2026-09-25T17:08:09.5000000Z [ci] test worktree_removal_refuses_stale_identity ... ok
2026-09-25T17:08:33.9621300Z [ci]      Running unittests src/lib.rs (target/debug/deps/gitturtle_preview-b8639df1640ae514)
2026-09-25T17:08:34.0000000Z [ci] test mermaid::tests::no_disk_cache ... ok
2026-09-25T17:08:34.0100000Z [ci] running 1 test
2026-09-25T17:08:34.0200000Z [ci] test mermaid::tests::no_disk_cache ... ok
2026-09-25T17:08:34.0441189Z [ci] test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 121 filtered out; finished in 0.04s
2026-09-25T17:08:36.0107916Z [ci]    Doc-tests gitturtle_core
2026-09-25T17:08:36.0200000Z [ci] test crates/git-core/src/lib.rs - paths::Path (line 12) ... ok
"""

NEXTEST = """\
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:00.0000000Z [ci]     Starting 7 tests across 4 binaries (1 test skipped)
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:01.0000000Z [ci]         PASS [   0.012s] (1/7) gitturtle::bin/gitturtle activity::tests::retains_nothing
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:01.0000000Z [ci]         PASS [   0.013s] (2/7) gitturtle::bin/gitturtle panics::tests::rejects
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:01.0000000Z [ci]         SKIP [         ] (─────) gitturtle::bin/gitturtle github::transport::tests::installed_cli
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:02.0000000Z [ci]         SLOW [> 60.000s] (───) gitturtle-core history::tests::pages
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:03.0000000Z [ci]         PASS [  61.000s] (3/7) gitturtle-core history::tests::pages
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:03.0000000Z [ci]         FAIL [   0.500s] (4/7) gitturtle-core::worktrees_reflog worktree_removal_refuses_stale_identity
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:03.0000000Z [ci]         PASS [   0.100s] (5/7) gitturtle-preview mermaid::tests::no_disk_cache
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:04.0000000Z [ci]      Summary [  61.600s] 5 tests run: 4 passed, 1 failed, 1 skipped
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:04.0000000Z [ci]         FAIL [   0.500s] (4/7) gitturtle-core::worktrees_reflog worktree_removal_refuses_stale_identity
"""

DOCTESTS = """\
   Doc-tests gitturtle_core
test crates/git-core/src/lib.rs - paths::Path (line 12) ... ok
"""


class ExtractionTests(unittest.TestCase):
    def test_libtest_log_keys_each_binary_and_separates_ignored_tests(self):
        executed, ignored = executed_tests.extract(CARGO_TEST.splitlines(keepends=True))
        self.assertEqual(executed, {
            "bin:gitturtle activity::tests::retains_nothing",
            "bin:gitturtle panics::tests::rejects",
            "lib:gitturtle_core history::tests::pages",
            "test:worktrees_reflog worktree_removal_refuses_stale_identity",
            "lib:gitturtle_preview mermaid::tests::no_disk_cache",
            "doc:gitturtle_core crates/git-core/src/lib.rs - paths::Path (line 12)",
        })
        self.assertEqual(ignored, {"bin:gitturtle github::transport::tests::installed_cli"})

    def test_nextest_and_doctest_logs_match_the_cargo_test_log(self):
        after = executed_tests.extract([*NEXTEST.splitlines(), *DOCTESTS.splitlines()])
        before = executed_tests.extract(CARGO_TEST.splitlines())
        result = executed_tests.compare(before, after)
        self.assertEqual(result["before"], {"executed": 6, "ignored": 1})
        self.assertEqual(result["after"], result["before"])
        self.assertFalse(any(result[key] for key in result if key.endswith(("_before", "_after"))))

    def test_nextest_binary_ids_use_cargo_crate_spelling(self):
        self.assertEqual(executed_tests.nextest_key("gitturtle-core"), "lib:gitturtle_core")
        self.assertEqual(executed_tests.nextest_key("gitturtle::bin/gitturtle"), "bin:gitturtle")
        self.assertEqual(executed_tests.nextest_key("gitturtle-core::partial-staging"), "test:partial_staging")
        self.assertEqual(executed_tests.nextest_key("demo::example/tour"), "example:tour")
        self.assertEqual(executed_tests.libtest_key(None, "benches/scan.rs", "target/debug/deps/scan-0123456789abcdef"), "bench:scan")

    def test_cli_reports_the_symmetric_difference_and_fails(self):
        missing = NEXTEST.replace("PASS [   0.100s] (5/7) gitturtle-preview", "SKIP [         ] (─────) gitturtle-preview")
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            paths = {}
            for name, text in {"before": CARGO_TEST, "after": missing, "doc": DOCTESTS}.items():
                paths[name] = Path(directory) / f"{name}.log"
                paths[name].write_text(text, encoding="utf-8")
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                status = executed_tests.main(["compare", "--before", str(paths["before"]),
                                              "--after", str(paths["after"]), str(paths["doc"]), "--json"])
            result = json.loads(output.getvalue())
            self.assertEqual(status, 1)
            self.assertEqual(result["executed_only_before"], ["lib:gitturtle_preview mermaid::tests::no_disk_cache"])
            self.assertEqual(result["ignored_only_after"], ["lib:gitturtle_preview mermaid::tests::no_disk_cache"])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                status = executed_tests.main(["compare", "--before", str(paths["before"]), "--after", str(paths["before"])])
            self.assertEqual(status, 0)
            self.assertTrue(output.getvalue().endswith("identical\n"))


if __name__ == "__main__":
    unittest.main()
