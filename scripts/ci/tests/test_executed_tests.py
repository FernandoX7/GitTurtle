"""Executed-test counts from retained libtest and nextest log formats; no network required."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import re
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

# `gh run view --log` output of one job. The SLOW, FAIL and Summary lines and the
# failure repeated after the Summary line are Quality job 133383935071's shapes.
NEXTEST = """\
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:00.0000000Z [ci]     Starting 5 tests across 4 binaries (1 test skipped)
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:01.0000000Z [ci]         PASS [   0.012s] (1/5) gitturtle::bin/gitturtle activity::tests::retains_nothing
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:01.0000000Z [ci]         PASS [   0.013s] (2/5) gitturtle::bin/gitturtle panics::tests::rejects
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:01.0000000Z [ci]         SKIP [         ] (───) gitturtle::bin/gitturtle github::transport::tests::installed_cli
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:02.0000000Z [ci]         SLOW [> 60.000s] (───) gitturtle-core history::tests::pages
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:03.0000000Z [ci]         PASS [  61.000s] (3/5) gitturtle-core history::tests::pages
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:03.0000000Z [ci]         FAIL [   0.500s] (4/5) gitturtle-core::worktrees_reflog worktree_removal_refuses_stale_identity
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:03.0000000Z [ci]         PASS [   0.100s] (5/5) gitturtle-preview mermaid::tests::no_disk_cache
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:04.0000000Z [ci] ────────────
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:04.0000000Z [ci]      Summary [  61.600s] 5 tests run: 4 passed (1 slow), 1 failed, 1 skipped
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:04.0000000Z [ci]         FAIL [   0.500s] (4/5) gitturtle-core::worktrees_reflog worktree_removal_refuses_stale_identity
Rust tests and Clippy · ubuntu-24.04\tLocked workspace tests with nextest\t2026-09-28T10:00:04.0000000Z [ci] error: test run failed
"""

DOCTESTS = """\
   Doc-tests gitturtle_core
test crates/git-core/src/lib.rs - paths::Path (line 12) ... ok
"""

# Lines kept from three runs of `gitturtle-preview` under cargo-nextest 0.9.146
# on 2026-11-18, each through `metrics.py measure` as the hosted job runs it:
# the whole crate, `--partition slice:1/2` and `--partition slice:2/2`. A test
# outside the slice prints the same SKIP line as `svg_bounded_mutation_probe`,
# which is marked `#[ignore]`; the Starting and Summary lines count both alike.
# `libdav1d_layouts_match_the_binding` prints its output on success, indented.
REAL_UNSHARDED = """\
[ci] ────────────
[ci]  Nextest run ID f4311741-15a8-4a63-bb4f-ceee8cc066ca with nextest profile: ci
[ci]     Starting 192 tests across 1 binary (1 test skipped)
[ci]         PASS [   0.008s] (  1/192) gitturtle-preview animation::tests::caps_frames_and_duration_and_cancels_before_more_decode
[ci]         PASS [   0.010s] (  2/192) gitturtle-preview animation::tests::applies_transparent_overlay_and_background_disposal_before_next_frame
[ci]         PASS [   0.112s] ( 35/192) gitturtle-preview avif::tests::libdav1d_layouts_match_the_binding
[ci]   stdout ───
[ci]
[ci]     running 1 test
[ci]     api 7
[ci]     test avif::tests::libdav1d_layouts_match_the_binding ... ok
[ci]
[ci]     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 192 filtered out; finished in 0.10s
[ci]
[ci]         SKIP [         ] (───────) gitturtle-preview tests::svg_bounded_mutation_probe
[ci]         PASS [   0.006s] (183/192) gitturtle-preview tests::thumbnail_preserves_aspect_and_has_no_transparent_color_bleed
[ci]         PASS [   3.079s] (191/192) gitturtle-preview animation::tests::rejects_corrupt_oversized_headers_and_bounds_aggregate_output
[ci] ────────────
[ci]      Summary [   3.910s] 192 tests run: 192 passed, 1 skipped
"""

REAL_SLICE_1 = """\
[ci] ────────────
[ci]  Nextest run ID f477c8d9-400c-47de-be5f-4763d1e2fadb with nextest profile: ci
[ci]     Starting 96 tests across 1 binary (97 tests skipped)
[ci]         SKIP [         ] (─────) gitturtle-preview animation::tests::caps_frames_and_duration_and_cancels_before_more_decode
[ci]         SKIP [         ] (─────) gitturtle-preview avif::tests::libdav1d_layouts_match_the_binding
[ci]         PASS [   0.010s] ( 1/96) gitturtle-preview animation::tests::applies_transparent_overlay_and_background_disposal_before_next_frame
[ci]         SKIP [         ] (─────) gitturtle-preview tests::svg_bounded_mutation_probe
[ci]         SKIP [         ] (─────) gitturtle-preview tests::thumbnail_preserves_aspect_and_has_no_transparent_color_bleed
[ci]         PASS [   1.303s] (95/96) gitturtle-preview animation::tests::rejects_corrupt_oversized_headers_and_bounds_aggregate_output
[ci] ────────────
[ci]      Summary [   3.263s] 96 tests run: 96 passed, 97 skipped
"""

REAL_SLICE_2 = """\
[ci] ────────────
[ci]  Nextest run ID 8b6e3b69-08bb-432d-899c-ae62251b35df with nextest profile: ci
[ci]     Starting 96 tests across 1 binary (97 tests skipped)
[ci]         SKIP [         ] (─────) gitturtle-preview animation::tests::applies_transparent_overlay_and_background_disposal_before_next_frame
[ci]         SKIP [         ] (─────) gitturtle-preview animation::tests::rejects_corrupt_oversized_headers_and_bounds_aggregate_output
[ci]         PASS [   0.012s] ( 1/96) gitturtle-preview animation::tests::caps_frames_and_duration_and_cancels_before_more_decode
[ci]         PASS [   0.132s] (32/96) gitturtle-preview avif::tests::libdav1d_layouts_match_the_binding
[ci]   stdout ───
[ci]
[ci]     running 1 test
[ci]     api 7
[ci]     test avif::tests::libdav1d_layouts_match_the_binding ... ok
[ci]
[ci]     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 192 filtered out; finished in 0.13s
[ci]
[ci]         SKIP [         ] (─────) gitturtle-preview tests::svg_bounded_mutation_probe
[ci]         PASS [   0.011s] (85/96) gitturtle-preview tests::thumbnail_preserves_aspect_and_has_no_transparent_color_bleed
[ci] ────────────
[ci]      Summary [   2.504s] 96 tests run: 96 passed, 97 skipped
"""

# The NEXTEST run's tests with the shard of `--partition slice:m/2` that runs
# each; 0 is the test marked `#[ignore]`, which no shard runs.
SHARDED = [
    ("gitturtle::bin/gitturtle activity::tests::retains_nothing", 1),
    ("gitturtle::bin/gitturtle panics::tests::rejects", 2),
    ("gitturtle::bin/gitturtle github::transport::tests::installed_cli", 0),
    ("gitturtle-core history::tests::pages", 1),
    ("gitturtle-core::worktrees_reflog worktree_removal_refuses_stale_identity", 2),
    ("gitturtle-preview mermaid::tests::no_disk_cache", 1),
]
REJECTS = "bin:gitturtle panics::tests::rejects"
PAGES = "lib:gitturtle_core history::tests::pages"
INSTALLED_CLI = "bin:gitturtle github::transport::tests::installed_cli"
REFLOG = "test:worktrees_reflog worktree_removal_refuses_stale_identity"

# What the hosted wrapper leaves of a module whose name reads like a
# credential's: three tests that run and one that is ignored, under one name
# (Quality job 133069788008 prints 28 such lines).
REDACTED_NAME = "gitturtle::bin/gitturtle github::transport::secret_service=[redacted]"
REDACTED = "bin:gitturtle github::transport::secret_service=[redacted]"
RETAINS = "gitturtle::bin/gitturtle activity::tests::retains_nothing"


def plural(count):
    return f"{count} test{'' if count == 1 else 's'}"


def nextest_log(tests, shard=None, *, prefix="[ci]"):
    """One nextest run in 0.9.146's shapes: every test of the list as its result or as a SKIP line.

    A test's owner is a shard number or a collection of them; without a shard
    every test with an owner runs. The Starting and Summary lines hold what the
    result lines add up to.
    """
    mine = [shard is None and owner != 0 or shard in (owner if isinstance(owner, tuple) else (owner,)) for _, owner in tests]
    ran, lines = sum(mine), []
    for (test, _), runs in zip(tests, mine):
        place = f"{sum(mine[:len(lines) + 1])}/{ran}" if runs else "───"
        lines.append(f"{prefix}         {'PASS [   0.012s]' if runs else 'SKIP [         ]'} ({place}) {test}\n")
    skipped = f" ({plural(len(tests) - ran)} skipped)" if len(tests) > ran else ""
    return (f"{prefix}     Starting {plural(ran)} across 4 binaries{skipped}\n" + "".join(lines) + f"{prefix} ────────────\n"
            f"{prefix}      Summary [   0.031s] {plural(ran)} run: {ran} passed, {len(tests) - ran} skipped\n")


def shard_log(shard, tests=None):
    """One shard's job log; shard 1's job also runs the doctests."""
    prefix = f"Rust tests · ubuntu-24.04 · {shard}/2\tLocked workspace tests with nextest\t2026-11-18T10:00:01.0000000Z [ci]"
    return nextest_log(SHARDED if tests is None else tests, shard, prefix=prefix) + (DOCTESTS if shard == 1 else "")


def owned(changes):
    """SHARDED with some tests' owners replaced, by the test's name."""
    return [(test, changes.get(test.split()[-1], owner)) for test, owner in SHARDED]


def excerpt(text):
    """A trimmed real log with its figures rewritten to the lines kept, since nextest's count the whole run."""
    lines = text.splitlines()
    ran = [index for index, line in enumerate(lines) if re.search(r"\] \(\s*\d+/\d+\) ", line)]
    skipped = sum(" SKIP [" in line for line in lines)
    for place, index in enumerate(ran, 1):
        lines[index] = re.sub(r"\(\s*\d+/\d+\)", f"({place}/{len(ran)})", lines[index])
    text = re.sub(r"Starting \d+ tests across 1 binary \(\d+ tests? skipped\)",
                  f"Starting {plural(len(ran))} across 1 binary ({plural(skipped)} skipped)", "\n".join(lines) + "\n")
    return re.sub(r"\d+ tests run: \d+ passed, \d+ skipped", f"{plural(len(ran))} run: {len(ran)} passed, {skipped} skipped", text)


def extract(text):
    return executed_tests.extract(text.splitlines(keepends=True))


class Comparison(unittest.TestCase):
    """The command over files: `before` and each of the `after` logs, by name."""

    def compare(self, after, *, before=NEXTEST + DOCTESTS, json_output=True):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            paths = []
            for name, text in {"before": before, **after}.items():
                paths.append(Path(directory) / f"{name}.log")
                paths[-1].write_text(text, encoding="utf-8")
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                status = executed_tests.main(["compare", "--before", str(paths[0]), "--after", *map(str, paths[1:]),
                                              *(["--json"] if json_output else [])])
            text = output.getvalue().replace(f"{directory}/", "")
            return status, (json.loads(text) if json_output else text)

    def shards(self, first=None, second=None, **options):
        return self.compare({"shard-1": shard_log(1, first), "shard-2": shard_log(2, second)}, **options)

    def assertNoDifference(self, result, *, but=()):
        for key in executed_tests.DIFFERENCES:
            if key not in but:
                self.assertFalse(result[key], key)


class ExtractionTests(Comparison):
    def test_libtest_log_keys_each_binary_and_separates_ignored_tests(self):
        log = extract(CARGO_TEST)
        self.assertEqual(log["libtest_ran"], {
            "bin:gitturtle activity::tests::retains_nothing",
            "bin:gitturtle panics::tests::rejects",
            PAGES,
            REFLOG,
            "lib:gitturtle_preview mermaid::tests::no_disk_cache",
            "doc:gitturtle_core crates/git-core/src/lib.rs - paths::Path (line 12)",
        })
        self.assertEqual(log["libtest_ignored"], {INSTALLED_CLI})
        self.assertEqual((log["nextest"], log["problems"], sum(log["ran"].values())), (None, [], 0))

    def test_nextest_and_doctest_logs_match_the_cargo_test_log(self):
        status, result = self.compare({"after": NEXTEST, "doc": DOCTESTS}, before=CARGO_TEST)
        self.assertEqual(status, 0)
        self.assertEqual((result["before"]["executed"], result["before"]["ignored"]), (6, 1))
        self.assertEqual((result["after"]["executed"], result["after"]["ignored"]), (6, 1))
        self.assertNoDifference(result)
        # Only one side holds a nextest run, so there are no run totals to compare.
        self.assertEqual((result["before"]["tests_run"], result["after"]["tests_run"]), (None, 5))

    def test_nextest_binary_ids_use_cargo_crate_spelling(self):
        self.assertEqual(executed_tests.nextest_key("gitturtle-core"), "lib:gitturtle_core")
        self.assertEqual(executed_tests.nextest_key("gitturtle::bin/gitturtle"), "bin:gitturtle")
        self.assertEqual(executed_tests.nextest_key("gitturtle-core::partial-staging"), "test:partial_staging")
        self.assertEqual(executed_tests.nextest_key("demo::example/tour"), "example:tour")
        self.assertEqual(executed_tests.libtest_key(None, "benches/scan.rs", "target/debug/deps/scan-0123456789abcdef"), "bench:scan")

    def test_cli_reports_the_difference_by_count_and_fails(self):
        missing = NEXTEST.replace("PASS [   0.100s] (5/5) gitturtle-preview", "SKIP [         ] (───) gitturtle-preview")
        missing = missing.replace("Starting 5 tests across 4 binaries (1 test skipped)", "Starting 4 tests across 4 binaries (2 tests skipped)")
        missing = missing.replace("5 tests run: 4 passed (1 slow), 1 failed, 1 skipped", "4 tests run: 3 passed (1 slow), 1 failed, 2 skipped")
        missing = missing.replace("/5)", "/4)")
        key = "lib:gitturtle_preview mermaid::tests::no_disk_cache"
        status, result = self.compare({"after": missing, "doc": DOCTESTS}, before=CARGO_TEST)
        self.assertEqual((status, result["unreconciled"]), (1, []))
        self.assertEqual(result["executed_less_after"], {key: {"before": 1, "after": 0, "before_logs": {"before.log": 1},
                                                               "after_logs": {"after.log": 0, "doc.log": 0}}})
        self.assertEqual(result["ignored_more_after"], {key: {"before": 0, "after": 1}})
        status, text = self.compare({"after": CARGO_TEST}, before=CARGO_TEST, json_output=False)
        self.assertEqual((status, text), (0, "before: 6 executed, 1 ignored\nafter: 6 executed, 1 ignored\nidentical\n"))

    def test_list_prints_one_line_for_each_result(self):
        tests = [(REDACTED_NAME, 1), (REDACTED_NAME, 1), (REDACTED_NAME, 0), (RETAINS, 1)]
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            path = Path(directory) / "job.log"
            path.write_text(nextest_log(tests), encoding="utf-8")
            output, errors = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                status = executed_tests.main(["list", str(path)])
            self.assertEqual((status, errors.getvalue()), (0, ""))
            self.assertEqual(output.getvalue(), "executed bin:gitturtle activity::tests::retains_nothing\n"
                                                f"executed {REDACTED}\nexecuted {REDACTED}\nignored {REDACTED}\n")
            path.write_text(nextest_log(tests).replace("3 tests run", "4 tests run"), encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(errors):
                status = executed_tests.main(["list", str(path)])
            self.assertEqual(status, 3)
            self.assertIn("job.log: result lines of finished tests: 3, tests run in its Summary line: 4 (1 fewer)\n", errors.getvalue())


class NextestOutputTests(Comparison):
    """What cargo-nextest 0.9.146 prints, from real runs with and without `--partition`."""

    def test_a_run_is_counted_from_its_starting_line_to_its_summary_line(self):
        log = extract(NEXTEST)
        self.assertEqual(log["problems"], [])
        self.assertEqual(log["nextest"], {"tests_run": 5, "passed": 4, "failed": 1, "skipped": 1, "other": {}})
        # The SLOW line is progress, and the failure repeated after the Summary
        # line is the same result: each test has one.
        self.assertEqual((log["ran"][PAGES], log["ran"][REFLOG], sum(log["ran"].values())), (1, 1, 5))
        self.assertEqual(dict(log["skipped"]), {INSTALLED_CLI: 1})

    def test_a_real_log_reconciles_and_a_trimmed_one_does_not(self):
        for text, ran, skipped in ((REAL_UNSHARDED, 5, 1), (REAL_SLICE_1, 2, 4), (REAL_SLICE_2, 3, 3)):
            log = extract(excerpt(text))
            self.assertEqual(log["problems"], [])
            self.assertEqual((log["nextest"]["tests_run"], log["nextest"]["passed"], log["nextest"]["skipped"]), (ran, ran, skipped))
            self.assertEqual((sum(log["ran"].values()), sum(log["skipped"].values())), (ran, skipped))
        # The lines as nextest printed them count 192 and 96 tests; the excerpts keep five and two.
        self.assertEqual(extract(REAL_UNSHARDED)["problems"], [
            "result lines of finished tests: 5, tests run in its Summary line: 192 (187 fewer)",
            "PASS lines: 5, passed in its Summary line: 192 (187 fewer)"])
        self.assertEqual(extract(REAL_SLICE_1)["problems"], [
            "result lines of finished tests: 2, tests run in its Summary line: 96 (94 fewer)",
            "PASS lines: 2, passed in its Summary line: 96 (94 fewer)",
            "SKIP lines: 4, skipped in its Summary line: 97 (93 fewer)"])

    def test_a_test_outside_the_slice_prints_like_an_ignored_test(self):
        probe = "lib:gitturtle_preview tests::svg_bounded_mutation_probe"
        caps = "lib:gitturtle_preview animation::tests::caps_frames_and_duration_and_cancels_before_more_decode"
        first, second = extract(excerpt(REAL_SLICE_1)), extract(excerpt(REAL_SLICE_2))
        self.assertEqual((first["skipped"][probe], first["skipped"][caps]), (1, 1))
        self.assertEqual((second["skipped"][probe], second["ran"][caps]), (1, 1))
        side = executed_tests.unite([("slice-1", first), ("slice-2", second)])
        self.assertEqual((dict(side["ignored"]), sum(side["executed"].values()), side["tests_run"]), ({probe: 1}, 5, 5))
        self.assertEqual((side["repeated"], side["problems"]), ({}, []))

    def test_what_a_test_prints_is_not_a_libtest_result(self):
        log = extract(excerpt(REAL_SLICE_2))
        self.assertEqual((log["libtest_ran"], log["libtest_ignored"]), (set(), set()))
        self.assertEqual(log["ran"]["lib:gitturtle_preview avif::tests::libdav1d_layouts_match_the_binding"], 1)

    def test_real_shard_logs_equal_the_real_unsharded_log(self):
        status, text = self.compare({"slice-1": excerpt(REAL_SLICE_1), "slice-2": excerpt(REAL_SLICE_2)},
                                    before=excerpt(REAL_UNSHARDED), json_output=False)
        self.assertEqual((status, text), (0, "before: 5 executed, 1 ignored\n"
                                             "after: 5 executed, 1 ignored (2 logs taken together)\n"
                                             "  slice-1.log: 2 executed, 4 skipped or ignored\n"
                                             "  slice-2.log: 3 executed, 3 skipped or ignored\n"
                                             "nextest tests run: 5 before, 5 after (slice-1.log 2, slice-2.log 3)\n"
                                             "identical\n"))


class ReconciliationTests(Comparison):
    """Each log against nextest's own Starting and Summary lines, whatever the comparison then says."""

    PASSED_REJECTS = "PASS [   0.012s] (1/2) gitturtle::bin/gitturtle panics::tests::rejects\n"
    FORGED_REJECTS = "[ci]             PASS [   0.012s] (1/1) gitturtle::bin/gitturtle panics::tests::rejects\n"

    def assertUnreconciled(self, after, problems, *, differences=()):
        status, result = self.compare(after)
        self.assertEqual((status, result["unreconciled"]), (3, problems))
        self.assertNoDifference(result, but=differences)
        for key in differences:
            self.assertTrue(result[key], key)
        status, text = self.compare(after, json_output=False)
        self.assertEqual(status, 3)
        self.assertIn(f"not reconciled with the runner's own totals ({len(problems)}):\n  " + "\n  ".join(problems) + "\n", text)
        self.assertTrue(text.endswith("\nunreconciled\n"))
        self.assertNotIn("identical", text)
        return result

    def test_a_missing_result_line_leaves_the_log_unreconciled_and_names_the_loss(self):
        shard = shard_log(2)
        self.assertIn(self.PASSED_REJECTS, shard)
        result = self.assertUnreconciled({"shard-1": shard_log(1), "shard-2": shard.replace(self.PASSED_REJECTS, "panicked\n")}, [
            "shard-2.log: result lines of finished tests: 1, tests run in its Summary line: 2 (1 fewer)",
            "shard-2.log: PASS lines: 1, passed in its Summary line: 2 (1 fewer)",
            "after: the logs do not list the same tests, so they are not the shards of one run on one platform: "
            f"1 key differs, first {REJECTS} (shard-1.log 1, shard-2.log 0)",
        ], differences=("executed_less_after", "ignored_more_after"))
        self.assertEqual((result["executed_less_after"][REJECTS]["before"], result["executed_less_after"][REJECTS]["after"]), (1, 0))

    def test_a_repeated_result_line_leaves_the_log_unreconciled_and_names_the_repeat(self):
        shard = shard_log(2).replace(self.PASSED_REJECTS, self.PASSED_REJECTS * 2)
        result = self.assertUnreconciled({"shard-1": shard_log(1), "shard-2": shard}, [
            "shard-2.log: result lines of finished tests: 3, tests run in its Summary line: 2 (1 more)",
            "shard-2.log: PASS lines: 3, passed in its Summary line: 2 (1 more)",
            "after: the logs do not list the same tests, so they are not the shards of one run on one platform: "
            f"1 key differs, first {REJECTS} (shard-1.log 1, shard-2.log 2)",
        ], differences=("executed_more_after",))
        self.assertEqual((result["executed_more_after"][REJECTS]["before"], result["executed_more_after"][REJECTS]["after"]), (1, 2))

    def test_a_forged_pass_line_cannot_stand_in_for_a_test_that_did_not_run(self):
        # The run has no `rejects`, and one of its tests prints the PASS line
        # `rejects` would have had, indented as nextest shows a test's output.
        run = nextest_log([test for test in SHARDED if not test[0].endswith("rejects")])
        forged = run.replace("[ci]      Summary", self.FORGED_REJECTS + "[ci]      Summary")
        self.assertEqual(extract(run)["problems"], [])
        result = self.assertUnreconciled({"after": forged + DOCTESTS}, [
            "after.log: result lines of finished tests: 5, tests run in its Summary line: 4 (1 more)",
            "after.log: PASS lines: 5, passed in its Summary line: 4 (1 more)",
        ])
        # Counted alone, the forged line makes every key equal the reference run's.
        self.assertEqual((result["before"]["executed"], result["after"]["executed"]), (6, 6))
        # After the Summary line nextest repeats the failures, so a line there is no result at all.
        self.assertEqual(extract(run + self.FORGED_REJECTS)["ran"][REJECTS], 0)
        self.assertEqual(extract(self.FORGED_REJECTS + run)["ran"][REJECTS], 0)

    def test_a_forged_summary_or_starting_line_is_a_second_run(self):
        lost = shard_log(2, owned({"panics::tests::rejects": 1}))
        run = ("[ci]     Starting 1 test across 1 binary\n" + self.FORGED_REJECTS
               + "[ci]      Summary [   0.012s] 1 test run: 1 passed, 0 skipped\n")
        starting, _, summary = run.splitlines(keepends=True)
        self.assertEqual(extract(run)["problems"], [])
        for forged, lines in ((lost + run, "2 Starting and 2 Summary lines of nextest"),
                              (run + lost, "2 Starting and 2 Summary lines of nextest"),
                              (lost + starting, "2 Starting and 1 Summary lines of nextest"),
                              (lost + summary, "1 Starting and 2 Summary lines of nextest, a Summary line outside a run"),
                              (summary + lost, "1 Starting and 2 Summary lines of nextest, a Summary line outside a run")):
            status, result = self.compare({"shard-1": shard_log(1), "shard-2": forged})
            self.assertEqual(status, 3)
            self.assertEqual(result["unreconciled"][0],
                             f"shard-2.log: {lines}: a log holds one run, from its Starting line to its Summary line")

    def test_a_run_that_did_not_finish_or_a_log_without_a_run_is_refused(self):
        cut = shard_log(2).split("[ci]      Summary")[0]
        result = self.assertUnreconciled({"shard-1": shard_log(1), "shard-2": cut}, [
            "shard-2.log: 1 Starting and 0 Summary lines of nextest: a log holds one run, from its Starting line to its Summary line",
        ])
        self.assertIsNone(result["after"]["logs"][1]["nextest"])
        self.assertUnreconciled({"shard-1": shard_log(1), "shard-2": shard_log(2), "notes": "PASS the salt\n"}, [
            "notes.log: neither a nextest run nor a libtest header: the log holds no test run"])

    def test_each_figure_of_the_summary_line_is_checked(self):
        summary = "5 tests run: 4 passed (1 slow), 1 failed, 1 skipped"
        for change, problems in (
            (("FAIL [   0.500s] (4/5)", "PASS [   0.500s] (4/5)"), ["PASS lines: 5, passed in its Summary line: 4 (1 more)",
                                                                    "FAIL lines: 0, failed in its Summary line: 1 (1 fewer)"]),
            (("SKIP [         ] (───) gitturtle::bin/gitturtle github", "note: gitturtle::bin/gitturtle github"),
             ["SKIP lines: 0, skipped in its Summary line: 1 (1 fewer)"]),
            (("PASS [   0.012s] (1/5)", "LEAK [   0.012s] (1/5)"), ["PASS lines: 3, passed in its Summary line: 4 (1 fewer); other statuses: LEAK 1"]),
            (("Starting 5 tests across 4 binaries (1 test skipped)", "Starting 6 tests across 4 binaries"),
             ["tests in its Starting line: 6, tests run in its Summary line: 5",
              "skipped in its Starting line: 0, skipped in its Summary line: 1"]),
            ((summary, "5 tests run: 4 passed; 1 failed"),
             ["a nextest line this tool cannot read: Summary [  61.600s] 5 tests run: 4 passed; 1 failed"]),
            ((summary, "4/5 tests run: 3 passed, 1 failed, 1 skipped"),
             ["a nextest line this tool cannot read: Summary [  61.600s] 4/5 tests run: 3 passed, 1 failed, 1 skipped"]),
            (("Starting 5 tests across 4 binaries (1 test skipped)", "Starting 5 tests across four binaries"),
             ["a nextest line this tool cannot read: Starting 5 tests across four binaries"]),
            # Not a shape taken from a real log: a figure this tool does not know is read, and the others still hold.
            ((summary, "5 tests run: 4 passed (1 slow), 1 timed out, 1 skipped"), ["FAIL lines: 1, failed in its Summary line: 0 (1 more)"]),
        ):
            self.assertIn(change[0], NEXTEST)
            self.assertEqual(extract(NEXTEST.replace(*change))["problems"], problems)
        log = extract(NEXTEST.replace(summary, "5 tests run: 4 passed (1 slow), 1 timed out, 1 skipped").replace("FAIL [   0.500s]", "TIMEOUT [ 120.000s]"))
        self.assertEqual((log["problems"], log["nextest"]["other"]), ([], {"timed out": 1}))

    def test_the_sides_run_totals_must_be_equal_whatever_else_a_log_holds(self):
        # nextest did not run `pages`, and a libtest-shaped line names it: every
        # key's counts are equal, and the run is one test short.
        after = nextest_log([test for test in SHARDED if not test[0].endswith("pages")]) + DOCTESTS + (
            "     Running unittests src/lib.rs (target/debug/deps/gitturtle_core-0123456789abcdef)\n"
            "test history::tests::pages ... ok\n")
        status, result = self.compare({"after": after})
        self.assertEqual((status, result["unreconciled"]), (1, []))
        self.assertNoDifference(result)
        self.assertEqual((result["before"]["tests_run"], result["after"]["tests_run"]), (5, 4))
        status, text = self.compare({"after": after}, json_output=False)
        self.assertEqual((status, text), (1, "before: 6 executed, 1 ignored\nafter: 6 executed, 1 ignored\n"
                                             "nextest tests run: 5 before, 4 after: 1 fewer after\ndifferent\n"))


class ShardTests(Comparison):
    """One unsharded "before" log against the logs of a run's two shards, taken together."""

    def test_two_shards_taken_together_equal_the_unsharded_run(self):
        status, result = self.shards()
        self.assertEqual((status, result["unreconciled"]), (0, []))
        self.assertNoDifference(result)
        self.assertEqual((result["before"]["executed"], result["before"]["ignored"], result["before"]["tests_run"]), (6, 1, 5))
        self.assertEqual((result["after"]["executed"], result["after"]["ignored"], result["after"]["tests_run"]), (6, 1, 5))
        # Each log's own counts: shard 1 ran three tests and the doctest and
        # lists the other three as skipped; shard 2 ran two and lists four.
        self.assertEqual([(log["log"], log["executed"], log["skipped_or_ignored"], log["nextest"]["tests_run"])
                          for log in result["after"]["logs"]], [("shard-1.log", 4, 3, 3), ("shard-2.log", 2, 4, 2)])
        # The libtest log of the same tests compares equal too.
        self.assertEqual(self.shards(before=CARGO_TEST)[0], 0)
        status, text = self.shards(json_output=False)
        self.assertEqual(status, 0)
        self.assertEqual(text, "before: 6 executed, 1 ignored\n"
                               "after: 6 executed, 1 ignored (2 logs taken together)\n"
                               "  shard-1.log: 4 executed, 3 skipped or ignored\n"
                               "  shard-2.log: 2 executed, 4 skipped or ignored\n"
                               "nextest tests run: 5 before, 5 after (shard-1.log 3, shard-2.log 2)\n"
                               "identical\n")

    def test_a_test_no_shard_ran_is_reported_as_lost(self):
        # The partition gave `rejects` to shard 2, and shard 2 skipped it too.
        lost = owned({"panics::tests::rejects": 1})
        status, result = self.shards(second=lost)
        self.assertEqual((status, result["unreconciled"]), (1, []))
        self.assertEqual(result["executed_less_after"], {REJECTS: {"before": 1, "after": 0, "before_logs": {"before.log": 1},
                                                                   "after_logs": {"shard-1.log": 0, "shard-2.log": 0}}})
        self.assertEqual(result["ignored_more_after"], {REJECTS: {"before": 0, "after": 1}})
        self.assertNoDifference(result, but=("executed_less_after", "ignored_more_after"))
        # A shard whose log is left out loses that shard's tests.
        status, result = self.compare({"shard-1": shard_log(1)})
        self.assertEqual(status, 1)
        self.assertEqual(list(result["executed_less_after"]), [REJECTS, REFLOG])
        status, text = self.shards(second=lost, json_output=False)
        self.assertEqual(status, 1)
        self.assertIn("nextest tests run: 5 before, 4 after (shard-1.log 3, shard-2.log 1): 1 fewer after\n"
                      f"executed less often after than before (lost) (1):\n  {REJECTS}: 1 before, 0 after (shard-1.log 0, shard-2.log 0)\n"
                      f"ignored more often after than before (1):\n  {REJECTS}: 0 before, 1 after\ndifferent\n", text)

    def test_a_test_two_shards_ran_is_reported_as_run_more_than_once(self):
        both = owned({"history::tests::pages": (1, 2)})
        status, result = self.shards(both, both)
        self.assertEqual((status, result["unreconciled"]), (1, []))
        self.assertEqual(result["executed_more_after"], {PAGES: {"before": 1, "after": 2, "before_logs": {"before.log": 1},
                                                                 "after_logs": {"shard-1.log": 1, "shard-2.log": 1}}})
        self.assertEqual(result["executed_more_than_listed_after"],
                         {PAGES: {"listed": 1, "executed": 2, "logs": {"shard-1.log": 1, "shard-2.log": 1}}})
        self.assertNoDifference(result, but=("executed_more_after", "executed_more_than_listed_after"))
        self.assertEqual((result["after"]["executed"], result["after"]["tests_run"]), (7, 6))
        status, text = self.shards(both, both, json_output=False)
        self.assertEqual(status, 1)
        self.assertIn("nextest tests run: 5 before, 6 after (shard-1.log 3, shard-2.log 3): 1 more after\n"
                      f"executed more often after than before (run more than once, or new) (1):\n"
                      f"  {PAGES}: 1 before, 2 after (shard-1.log 1, shard-2.log 1)\n"
                      f"executed more often than the after logs list it (run more than once) (1):\n"
                      f"  {PAGES}: listed 1, executed 2 (shard-1.log 1, shard-2.log 1)\ndifferent\n", text)
        # Both sides partitioned alike: the sides are equal, and each still ran the test twice.
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            paths = [Path(directory) / f"shard-{shard}.log" for shard in (1, 2)]
            for shard, path in enumerate(paths, 1):
                path.write_text(shard_log(shard, both), encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()) as output:
                status = executed_tests.main(["compare", "--before", *map(str, paths), "--after", *map(str, paths), "--json"])
            result = json.loads(output.getvalue())
            self.assertEqual((status, result["executed_more_after"]), (1, {}))
            self.assertEqual(list(result["executed_more_than_listed_before"]), [PAGES])

    def test_an_ignored_test_is_one_the_shards_list_and_none_ran(self):
        logs = [(f"shard-{number}", extract(shard_log(number))) for number in (1, 2)]
        # Each shard's own log marks the other shard's tests as skipped as well.
        self.assertEqual((logs[0][1]["skipped"][REJECTS], logs[1][1]["skipped"][PAGES]), (1, 1))
        side = executed_tests.unite(logs)
        self.assertEqual(dict(side["ignored"]), {INSTALLED_CLI: 1})
        self.assertEqual((sum(side["executed"].values()), side["repeated"], side["problems"]), (6, {}, []))
        # A test the unsharded run ignored and a shard ran is a difference on both counts.
        status, result = self.shards(second=owned({"github::transport::tests::installed_cli": 2}))
        self.assertEqual(status, 1)
        self.assertEqual((list(result["ignored_less_after"]), list(result["executed_more_after"])), ([INSTALLED_CLI], [INSTALLED_CLI]))
        self.assertNoDifference(result, but=("ignored_less_after", "executed_more_after"))

    def test_shards_of_different_test_lists_are_not_one_run(self):
        # Shard 2 is another platform's: its list lacks one test.
        other = [test for test in SHARDED if test[0] != RETAINS]
        status, result = self.shards(second=other)
        self.assertEqual(status, 3)
        self.assertEqual(result["unreconciled"], [
            "after: the logs do not list the same tests, so they are not the shards of one run on one platform: "
            "1 key differs, first bin:gitturtle activity::tests::retains_nothing (shard-1.log 1, shard-2.log 0)"])


class RedactedNameTests(Comparison):
    """Results that share a name are compared by count: three executed and one ignored under one key."""

    UNSHARDED = [(REDACTED_NAME, 1), (REDACTED_NAME, 1), (REDACTED_NAME, 1), (REDACTED_NAME, 0), (RETAINS, 1)]

    def split(self, *owners, **options):
        tests = [(test, owner) for (test, _), owner in zip(self.UNSHARDED, (*owners, 0, 2))]
        return self.compare({"shard-1": nextest_log(tests, 1), "shard-2": nextest_log(tests, 2)},
                            before=nextest_log(self.UNSHARDED), **options)

    def test_results_under_one_name_are_counted(self):
        log = extract(nextest_log(self.UNSHARDED))
        self.assertEqual((log["problems"], log["ran"][REDACTED], log["skipped"][REDACTED]), ([], 3, 1))
        self.assertEqual((log["nextest"]["tests_run"], len(log["ran"])), (4, 2))

    def test_a_correct_split_of_one_name_is_identical(self):
        status, result = self.split(1, 2, 1)
        self.assertEqual((status, result["unreconciled"]), (0, []))
        self.assertNoDifference(result)
        self.assertEqual((result["after"]["executed"], result["after"]["ignored"], result["after"]["tests_run"]), (4, 1, 4))
        self.assertEqual([(log["executed"], log["skipped_or_ignored"]) for log in result["after"]["logs"]], [(2, 3), (2, 3)])

    def test_a_split_that_loses_tests_of_one_name_says_how_many(self):
        # Two of the three run in no shard, and the name still appears as executed.
        status, result = self.split(1, 0, 0)
        self.assertEqual((status, result["unreconciled"]), (1, []))
        self.assertEqual(result["executed_less_after"], {REDACTED: {"before": 3, "after": 1, "before_logs": {"before.log": 3},
                                                                    "after_logs": {"shard-1.log": 1, "shard-2.log": 0}}})
        self.assertEqual(result["ignored_more_after"], {REDACTED: {"before": 1, "after": 3}})
        self.assertNoDifference(result, but=("executed_less_after", "ignored_more_after"))
        status, text = self.split(1, 0, 0, json_output=False)
        self.assertEqual((status, text), (1, "before: 4 executed, 1 ignored\n"
                                             "after: 2 executed, 3 ignored (2 logs taken together)\n"
                                             "  shard-1.log: 1 executed, 4 skipped or ignored\n"
                                             "  shard-2.log: 1 executed, 4 skipped or ignored\n"
                                             "nextest tests run: 4 before, 2 after (shard-1.log 1, shard-2.log 1): 2 fewer after\n"
                                             "executed less often after than before (lost) (1):\n"
                                             f"  {REDACTED}: 3 before, 1 after (shard-1.log 1, shard-2.log 0)\n"
                                             "ignored more often after than before (1):\n"
                                             f"  {REDACTED}: 1 before, 3 after\n"
                                             "different\n"))

    def test_a_split_that_repeats_a_test_of_one_name_says_how_many(self):
        status, result = self.split(1, 2, (1, 2))
        self.assertEqual((status, result["unreconciled"]), (1, []))
        self.assertEqual((result["executed_more_after"][REDACTED]["before"], result["executed_more_after"][REDACTED]["after"]), (3, 4))
        self.assertEqual(result["executed_more_after"][REDACTED]["after_logs"], {"shard-1.log": 2, "shard-2.log": 2})
        # Four results are listed under the name and four ran: only the count against the other side shows the repeat.
        self.assertNoDifference(result, but=("executed_more_after", "ignored_less_after"))
        self.assertEqual(result["ignored_less_after"], {REDACTED: {"before": 1, "after": 0}})


class TerminalTests(Comparison):
    """Names and unread lines come from logs a pull request's test output can shape; the text report prints no control."""

    # OSC 52 sets the clipboard of a terminal that reads it; then an 8-bit CSI,
    # DEL, a right-to-left override and a backslash that must stay told apart.
    OSC_52 = "\x1b]52;c;cm0gLXJmIH4=\x07"
    HOSTILE = f"gitturtle-core history::{OSC_52}pages\x9b2J\x7f\u202e\\x1b"
    ESCAPED = "lib:gitturtle_core history::\\x1b]52;c;cm0gLXJmIH4=\\x07pages\\x9b2J\\x7f\\u202e\\\\x1b"

    def assertPrintable(self, text):
        self.assertEqual([character for character in text if not character.isprintable()], ["\n"] * text.count("\n"))

    def test_the_text_report_escapes_an_osc_52_sequence_in_a_result_line(self):
        after = nextest_log([(self.HOSTILE, 1), (RETAINS, 1)])
        self.assertIn(self.OSC_52, after)
        status, text = self.compare({"after": after}, before=nextest_log([(RETAINS, 1)]), json_output=False)
        self.assertEqual((status, text), (1, "before: 1 executed, 0 ignored\nafter: 2 executed, 0 ignored\n"
                                             "nextest tests run: 1 before, 2 after: 1 more after\n"
                                             "executed more often after than before (run more than once, or new) (1):\n"
                                             f"  {self.ESCAPED}: 0 before, 1 after\ndifferent\n"))
        self.assertPrintable(text)
        # `--json` already escaped them, and keeps the name itself.
        status, result = self.compare({"after": after}, before=nextest_log([(RETAINS, 1)]))
        self.assertEqual(list(result["executed_more_after"]), [executed_tests.nextest_key("gitturtle-core") + " " + self.HOSTILE.split(" ", 1)[1]])

    def test_an_unread_line_and_a_listing_are_escaped_too(self):
        # A Summary line the tool cannot read is quoted in the report.
        unread = nextest_log([(RETAINS, 1)]).replace("0 skipped\n", f"0 skipped{self.OSC_52}\n")
        status, text = self.compare({"after": unread}, before=nextest_log([(RETAINS, 1)]), json_output=False)
        self.assertEqual(status, 3)
        self.assertIn("after.log: a nextest line this tool cannot read: Summary [   0.031s] 1 test run: 1 passed, "
                      "0 skipped\\x1b]52;c;cm0gLXJmIH4=\\x07\n", text)
        self.assertPrintable(text)
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            path = Path(directory) / "job.log"
            path.write_text(nextest_log([(self.HOSTILE, 1), (RETAINS, 0)]).replace("1 skipped\n", f"1 skipped{self.OSC_52}\n"), encoding="utf-8")
            output, errors = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                status = executed_tests.main(["list", str(path)])
            self.assertEqual((status, output.getvalue()), (3, f"executed {self.ESCAPED}\nignored bin:gitturtle activity::tests::retains_nothing\n"))
            self.assertIn("job.log: a nextest line this tool cannot read: Summary [   0.031s] 1 test run: 1 passed, "
                          "1 skipped\\x1b]52;c;cm0gLXJmIH4=\\x07\n", errors.getvalue())
            self.assertPrintable(errors.getvalue())


if __name__ == "__main__":
    unittest.main()
