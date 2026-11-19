"""The tooling job's side-by-side step: the deal, its proof, the driver's output and the workflow's shape."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[3]
DRIVER = ROOT / "scripts/ci/tooling_suites.py"
SPEC = importlib.util.spec_from_file_location("ci_tooling_suites", DRIVER)
suites = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(suites)

STEP = 'python3 scripts/ci/tooling_suites.py run --directory "$RUNNER_TEMP/ci-metrics"'
# Small suites for the driver to run; every wait in them is for an event, never for a guessed time.
FIXTURES = {
    # A package the groups are dealt from: seven tests, sorted a1 a2 a3 a4 b1 b2 b3.
    "dealt/pkg/__init__.py": "",
    "dealt/pkg/test_alpha.py": """
        import os
        import unittest


        def note(event):
            with open(os.environ["TOOLING_SUITES_TEST_EVENTS"], "a", encoding="utf-8") as stream:
                stream.write(f"{os.getpid()} {event}\\n")


        def setUpModule():
            note("module-up")


        def tearDownModule():
            note("module-down")


        class Alpha(unittest.TestCase):
            @classmethod
            def setUpClass(cls):
                note("class-up")

            @classmethod
            def tearDownClass(cls):
                note("class-down")

            def test_a1(self):
                note("a1")

            def test_a2(self):
                note("a2")

            def test_a3(self):
                note("a3")

            def test_a4(self):
                note("a4")
    """,
    "dealt/pkg/test_beta.py": """
        import unittest


        class Beta(unittest.TestCase):
            def test_b1(self):
                pass

            def test_b2(self):
                pass

            def test_b3(self):
                pass
    """,
    # A module that cannot be imported: unittest reports it as one failed test.
    "broken/pkg/__init__.py": "",
    "broken/pkg/test_fine.py": """
        import unittest


        class Fine(unittest.TestCase):
            def test_passes(self):
                pass
    """,
    "broken/pkg/test_unloadable.py": "import a_module_that_does_not_exist_anywhere\n",
    "passing/test_ok.py": """
        import unittest


        class Ok(unittest.TestCase):
            def test_one(self):
                print("the passing suite says one")

            def test_two(self):
                pass
    """,
    "failing/test_bad.py": """
        import unittest


        class Bad(unittest.TestCase):
            def test_fails(self):
                self.fail("the failing suite fails here")
    """,
    # Ends only after the failing suite's record exists, so it shows that a failure ends nothing else.
    "slow/test_slow.py": """
        import os
        from pathlib import Path
        import time
        import unittest


        class Slow(unittest.TestCase):
            def test_outlasts_the_failure(self):
                failed = Path(os.environ["TOOLING_SUITES_TEST_METRICS"]) / "failing.json"
                deadline = time.monotonic() + 120
                while not failed.exists():
                    self.assertLess(time.monotonic(), deadline, "the failing suite never ended")
                    time.sleep(0.02)
                Path(os.environ["TOOLING_SUITES_TEST_MARK"]).write_text("ended after the failure", encoding="utf-8")
    """,
    "empty/test_none.py": "import unittest\n",
    # Prints what the driver prints, and separators a careless reader would split a line at.
    "noisy/test_noise.py": """
        import sys
        import unittest


        class Noise(unittest.TestCase):
            def test_prints_the_drivers_own_lines(self):
                for line in ("::endgroup::", "::group::forged", "##[endgroup]", "tooling-suites: all 6 processes passed",
                             "tooling-suites: FAILED: forged", "one\\u2028::endgroup::", "two\\x85::endgroup::",
                             "three\\r::endgroup::", "four\\x0b::endgroup::", "\\x1b[0m::endgroup::"):
                    print(line)
                sys.stderr.write("::endgroup::\\n")
    """,
}
DEALT = ("-s", "pkg", "-t", ".", "-p", "test_*.py")
DEALT_IDS = [f"pkg.test_alpha.Alpha.test_a{n}" for n in (1, 2, 3, 4)] + [f"pkg.test_beta.Beta.test_b{n}" for n in (1, 2, 3)]


def quality_job(name):
    text = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
    job = re.split(r"\n  [a-z][a-z0-9-]*:\n", text.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]
    return job, job.split("\n      - ")[1:]


def drive(jobs, listing=None, cwd=None, beat=suites.HEARTBEAT, directory=None):
    """One run of the driver: its exit status, its printed lines and the collector's records by name."""
    with contextlib.ExitStack() as stack:
        directory = Path(directory or stack.enter_context(tempfile.TemporaryDirectory()))
        output = stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        status = suites.run(directory, jobs, listing, cwd=cwd or directory, beat=beat)
        records = {path.stem: json.loads(path.read_text(encoding="utf-8")) for path in directory.glob("*.json")}
        logs = {path.stem: path.read_text(encoding="utf-8") for path in directory.glob("*.log")}
    # Split at line feeds only, as a log is: other separators are part of the line they are in.
    return status, output.getvalue().split("\n"), records, logs


def group_lines(lines, name):
    """The lines printed inside one process's log group."""
    start = lines.index(f"::group::{name}")
    return lines[start + 1:lines.index("::endgroup::", start)]


class Fixtures(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixtures = Path(cls.enterClassContext(tempfile.TemporaryDirectory()))
        for name, source in FIXTURES.items():
            path = cls.fixtures / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(textwrap.dedent(source).lstrip("\n"), encoding="utf-8")

    @classmethod
    def suite(cls, name, folder=None):
        """A job that runs one fixture folder as the job's other suites are run."""
        return suites.Job(name, (sys.executable, "-B", "-m", "unittest", "discover", "-s",
                                 str(cls.fixtures / (folder or name)), "-p", "test_*.py"), None)


class DealTests(unittest.TestCase):
    IDS = ["m.B.test_2", "m.A.test_1", "n.C.test_1", "m.B.test_1", "m.A.test_3", "m.A.test_2", "n.C.test_2"]

    def test_each_group_takes_every_third_sorted_id_in_turn(self):
        ordered = sorted(self.IDS)
        groups = [suites.deal(self.IDS, index, 3) for index in range(3)]
        self.assertEqual(groups, [[ordered[0], ordered[3], ordered[6]], [ordered[1], ordered[4]], [ordered[2], ordered[5]]])
        # Sorted inside a group, so a class's and a module's tests stay next to each other.
        self.assertEqual(groups, [sorted(group) for group in groups])

    def test_the_groups_are_disjoint_and_together_hold_every_id(self):
        for total in range(0, 20):
            identifiers = [f"m.T.test_{n:02}" for n in reversed(range(total))]
            groups = [suites.deal(identifiers, index, 3) for index in range(3)]
            with self.subTest(total=total):
                self.assertEqual(sorted(identifier for group in groups for identifier in group), sorted(identifiers))
                self.assertEqual(sum(map(len, groups)), len(set().union(*groups)))
                self.assertLessEqual(max(map(len, groups)) - min(map(len, groups)), 1)

    def test_a_new_test_is_dealt_without_a_list_and_moves_only_the_tests_after_it(self):
        before = {identifier: index for index in range(3) for identifier in suites.deal(self.IDS, index, 3)}
        after = {identifier: index for index in range(3) for identifier in suites.deal([*self.IDS, "m.B.test_0"], index, 3)}
        self.assertEqual(set(after), {*self.IDS, "m.B.test_0"})
        for identifier, index in before.items():
            # What sorts before the new test keeps its group; what sorts after it takes the next one.
            self.assertEqual(after[identifier], index if identifier < "m.B.test_0" else (index + 1) % 3, identifier)

    def test_the_tests_themselves_are_dealt_by_their_ids(self):
        class Case(unittest.TestCase):
            def test_b(self):
                pass

            def test_a(self):
                pass

            def test_c(self):
                pass

        nested = unittest.TestSuite([unittest.TestSuite([Case("test_c"), Case("test_b")]), Case("test_a")])
        kept = suites.deal(suites.flatten(nested), 0, 2, key=lambda test: test.id())
        self.assertEqual([test.id().rsplit(".", 1)[1] for test in kept], ["test_a", "test_c"])


class ProofTests(unittest.TestCase):
    GROUPS = {"one": ["t.a", "t.d"], "two": ["t.b", "t.e"], "three": ["t.c"]}
    FOUND = ["t.a", "t.b", "t.c", "t.d", "t.e"]

    def test_groups_that_hold_every_discovered_id_once_are_proven(self):
        self.assertEqual(suites.prove(self.FOUND, self.GROUPS), [])
        # The ids, not their number: the same count of other tests proves nothing.
        self.assertEqual(suites.prove(["t.a", "t.b", "t.c", "t.d", "t.x"], self.GROUPS),
                         [("discovered, in no group", "t.x"), ("in two, not discovered", "t.e")])

    def test_a_short_group_fails_with_the_missing_ids_named(self):
        self.assertEqual(suites.prove(self.FOUND, {**self.GROUPS, "two": ["t.b"]}), [("discovered, in no group", "t.e")])
        self.assertEqual(suites.prove(self.FOUND, {**self.GROUPS, "three": []}), [("discovered, in no group", "t.c")])

    def test_an_id_in_two_groups_fails_with_the_id_and_both_groups_named(self):
        self.assertEqual(suites.prove(self.FOUND, {**self.GROUPS, "three": ["t.c", "t.d"]}),
                         [("more than once, in one and three", "t.d")])
        self.assertEqual(suites.prove(self.FOUND, {**self.GROUPS, "three": ["t.c", "t.c"]}),
                         [("more than once, in three and three", "t.c")])

    def test_a_missing_record_or_discovery_proves_nothing(self):
        self.assertEqual(suites.prove(self.FOUND, {**self.GROUPS, "three": None}),
                         [("three left no readable record of its test ids", None), ("discovered, in no group", "t.c")])
        self.assertEqual(suites.prove(None, self.GROUPS),
                         [("the driver's own discovery left no readable list of test ids", None)])
        # Two empty listings agree with each other and prove nothing.
        self.assertEqual(suites.prove([], {"one": [], "two": []}), [("the driver's own discovery found no test", None)])
        self.assertEqual(suites.prove(["t.a", "t.a"], {"one": ["t.a"]}), [("discovered twice", "t.a")])

    def test_a_record_is_read_only_when_it_is_a_list_of_ids(self):
        with tempfile.TemporaryDirectory() as directory:
            record = Path(directory) / "record.json"
            self.assertIsNone(suites.read_record(record))
            suites.write_record(record, ["t.a", "t.b"])
            self.assertEqual(suites.read_record(record), ["t.a", "t.b"])
            self.assertEqual(sorted(path.name for path in Path(directory).iterdir()), ["record.json"])
            for text in ("", "{", "[]", '{"ids": "t.a"}', '{"ids": [1]}', '{"other": []}'):
                record.write_text(text, encoding="utf-8")
                self.assertIsNone(suites.read_record(record), text)


class GroupRunTests(Fixtures):
    """The agent-loop groups, on a small package instead of the real suite."""

    def dealt(self, count, folder="dealt"):
        records = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.events = records / "events"
        self.enterContext(mock.patch.dict(os.environ, {"TOOLING_SUITES_TEST_EVENTS": str(self.events)}))
        return suites.plan(records, discovery=DEALT, count=count, suites=()), self.fixtures / folder

    def test_two_groups_run_every_test_once_with_its_fixtures_and_the_step_passes(self):
        (jobs, listing), cwd = self.dealt(2)
        status, lines, records, _ = drive(jobs, listing, cwd)
        self.assertEqual(status, 0, lines)
        self.assertEqual([suites.read_record(job.record) for job in jobs], [DEALT_IDS[0::2], DEALT_IDS[1::2]])
        self.assertEqual(suites.read_record(listing.record), DEALT_IDS)
        self.assertIn("tooling-suites: the 2 agent-loop groups hold the 7 discovered tests, each once (4 + 3)", lines)
        self.assertEqual(lines[-3:], ["tooling-suites: the 2 agent-loop groups hold the 7 discovered tests, each once (4 + 3)",
                                      "tooling-suites: all 2 processes passed", ""])
        # unittest's own run and words, per group, and the collector's record of each.
        self.assertIn("[ci] Ran 4 tests in ", "\n".join(group_lines(lines, "agent-tooling-1-of-2")))
        self.assertIn("[ci] Ran 3 tests in ", "\n".join(group_lines(lines, "agent-tooling-2-of-2")))
        self.assertEqual({name: record["exit_code"] for name, record in records.items()},
                         {"agent-tooling-1-of-2": 0, "agent-tooling-2-of-2": 0})
        # The driver's own discovery leaves no measurement behind.
        self.assertNotIn("agent-tooling-discovery", "\n".join(lines))
        # In each process the module and the class are set up before its share of the class's
        # tests and torn down after it.
        by_process = {}
        for line in self.events.read_text(encoding="utf-8").splitlines():
            process, event = line.split()
            by_process.setdefault(process, []).append(event)
        self.assertEqual(sorted(by_process.values()),
                         [["module-up", "class-up", "a1", "a3", "class-down", "module-down"],
                          ["module-up", "class-up", "a2", "a4", "class-down", "module-down"]])

    def test_a_group_started_twice_and_a_group_not_started_fail_the_step_with_every_id_named(self):
        (jobs, listing), cwd = self.dealt(2)
        first, second = jobs
        # The second process deals itself the first one's share: those ids are held twice, its own by no one.
        again = second._replace(command=tuple("0" if previous == "--index" else argument
                                              for previous, argument in zip((None, *second.command), second.command)))
        self.assertNotEqual(again.command, second.command)
        status, lines, records, _ = drive([first, again], listing, cwd)
        self.assertEqual(status, 1)
        # Both processes passed on their own; only the proof fails the step.
        self.assertEqual({record["exit_code"] for record in records.values()}, {0})
        self.assertEqual([line for line in lines if "FAILED" in line],
                         [f"tooling-suites: PROOF FAILED: discovered, in no group: {identifier}" for identifier in DEALT_IDS[1::2]]
                         + ["tooling-suites: PROOF FAILED: more than once, in agent-tooling-1-of-2 and agent-tooling-2-of-2: "
                            + identifier for identifier in DEALT_IDS[0::2]])
        self.assertNotIn("tooling-suites: all 2 processes passed", lines)

    def test_a_module_that_cannot_be_imported_fails_its_group_as_a_failed_test(self):
        (jobs, listing), cwd = self.dealt(2, "broken")
        status, lines, records, _ = drive(jobs, listing, cwd)
        self.assertEqual(status, 1)
        # unittest's load error is a test like any other: dealt, run and failed, and the proof holds.
        self.assertEqual([suites.read_record(job.record) for job in jobs],
                         [["pkg.test_fine.Fine.test_passes"], ["unittest.loader._FailedTest.pkg.test_unloadable"]])
        self.assertEqual({name: record["exit_code"] for name, record in records.items()},
                         {"agent-tooling-1-of-2": 0, "agent-tooling-2-of-2": 1})
        self.assertEqual([line for line in lines if "FAILED" in line and line.startswith("tooling-suites: ")],
                         ["tooling-suites: FAILED: agent-tooling-2-of-2 exited 1"])
        self.assertIn("a_module_that_does_not_exist_anywhere", "\n".join(group_lines(lines, "agent-tooling-2-of-2")))

    def test_a_group_refuses_a_place_it_does_not_have(self):
        with tempfile.TemporaryDirectory() as directory:
            done = subprocess.run([sys.executable, "-B", str(DRIVER), "group", "--index", "2", "--of", "2", "--record",
                                   str(Path(directory) / "record.json"), *DEALT], cwd=self.fixtures / "dealt",
                                  stdin=subprocess.DEVNULL, capture_output=True, text=True)
            self.assertEqual((done.returncode, os.listdir(directory)), (2, []), done.stderr)
            self.assertIn("--index must be at least 0 and less than --of", done.stderr)


class DriverTests(Fixtures):
    """Three processes, the slowest listed first and one failing: nothing is ended early or printed out of turn."""

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.metrics = Path(cls.enterClassContext(tempfile.TemporaryDirectory()))
        cls.mark = cls.metrics / "slow-ended"
        cls.enterClassContext(mock.patch.dict(os.environ, {"TOOLING_SUITES_TEST_METRICS": str(cls.metrics),
                                                           "TOOLING_SUITES_TEST_MARK": str(cls.mark)}))
        cls.jobs = [cls.suite("slow"), cls.suite("failing"), cls.suite("passing")]
        cls.status, cls.lines, cls.records, cls.logs = drive(cls.jobs, beat=0.05, directory=cls.metrics)

    def test_a_failing_process_fails_the_step_after_the_others_ran_to_their_end(self):
        self.assertEqual(self.status, 1)
        # The slow suite ends only once the failing one's record is written, and it did end and pass.
        self.assertEqual(self.mark.read_text(encoding="utf-8"), "ended after the failure")
        self.assertEqual({name: record["exit_code"] for name, record in self.records.items()},
                         {"slow": 0, "failing": 1, "passing": 0})
        self.assertEqual([line for line in self.lines if line.startswith("tooling-suites: FAILED")],
                         ["tooling-suites: FAILED: failing exited 1"])
        self.assertNotIn("tooling-suites: all 3 processes passed", self.lines)

    def test_output_is_printed_whole_in_the_listed_order_whatever_ended_first(self):
        self.assertGreater(self.records["slow"]["elapsed_seconds"], 0)
        marks = [line for line in self.lines if line.startswith("::")]
        self.assertEqual(marks, ["::group::slow", "::endgroup::", "::group::failing", "::endgroup::",
                                 "::group::passing", "::endgroup::"])
        # Each group holds its own process's output and nothing of another's.
        self.assertIn("[ci] the passing suite says one", group_lines(self.lines, "passing"))
        self.assertIn("the failing suite fails here", "\n".join(group_lines(self.lines, "failing")))
        for name in ("slow", "passing"):
            self.assertNotIn("the failing suite fails here", "\n".join(group_lines(self.lines, name)))
        # Nothing of a process is printed before the last one ended: the groups follow every heartbeat.
        first_group = self.lines.index("::group::slow")
        beats = [index for index, line in enumerate(self.lines) if re.match(r"tooling-suites: \+\d+ s: ", line)]
        self.assertTrue(beats)
        self.assertLess(max(beats), first_group)
        self.assertRegex(self.lines[beats[0]], r"^tooling-suites: \+\d+ s: slow (running \(\d+ bytes of output\)|exit 0 after )")

    def test_each_process_keeps_the_collectors_log_and_record_and_the_log_holds_the_same_lines(self):
        self.assertEqual(sorted(path.name for path in self.metrics.iterdir() if path != self.mark),
                         ["failing.json", "failing.log", "passing.json", "passing.log", "slow.json", "slow.log"])
        for job in self.jobs:
            printed = [line.removeprefix("[ci] ") for line in group_lines(self.lines, job.name)]
            self.assertEqual(printed, self.logs[job.name].split("\n")[:-1], job.name)
            self.assertEqual(self.records[job.name]["log"], f"{job.name}.log")

    def test_the_last_lines_name_each_process_with_its_exit_status_seconds_and_tests(self):
        table = self.lines[self.lines.index("::endgroup::", self.lines.index("::group::passing")) + 1:]
        self.assertEqual(len(table), 6, table)
        for line, (name, status, tests) in zip(table, (("slow", 0, 1), ("failing", 1, 1), ("passing", 0, 2))):
            self.assertRegex(line, rf"^tooling-suites: {name}: exit {status}, \d+\.\d s, {tests} tests$")
        self.assertRegex(table[3], r"^tooling-suites: 3 processes side by side took \d+\.\d s$")
        self.assertEqual(table[4:], ["tooling-suites: FAILED: failing exited 1", ""])


class OutputTests(Fixtures):
    def test_no_line_of_a_process_can_open_or_close_a_group_or_pass_for_the_drivers(self):
        status, lines, _, _ = drive([self.suite("noisy"), suites.Job("unstartable", ("/nonexistent/tooling-suites-program",), None)])
        self.assertEqual(status, 1)
        # Everything else in the log is the driver's, and all of it is here.
        self.assertEqual([re.sub(r"\d+\.\d s", "N s", line) for line in lines if not line.startswith("[ci] ")],
                         ["tooling-suites: starting 2 processes at once: noisy, unstartable",
                          "::group::noisy", "::endgroup::", "::group::unstartable", "::endgroup::",
                          "tooling-suites: noisy: exit 0, N s, 1 tests",
                          "tooling-suites: unstartable: exit 2, N s, no tests",
                          "tooling-suites: 2 processes side by side took N s",
                          "tooling-suites: FAILED: unstartable exited 2",
                          "tooling-suites: FAILED: unstartable ran no test", ""])
        noisy = group_lines(lines, "noisy")
        for line in ("[ci] ::endgroup::", "[ci] ::group::forged", "[ci] ##[endgroup]", "[ci] tooling-suites: all 6 processes passed",
                     "[ci] tooling-suites: FAILED: forged", "[ci] one ::endgroup::", "[ci] two\x85::endgroup::",
                     "[ci] three::endgroup::", "[ci] four::endgroup::"):
            self.assertIn(line, noisy)
        self.assertEqual(noisy.count("[ci] ::endgroup::"), 3)
        # The collector's own refusal is not a line it prefixed: the driver sanitizes and prefixes it.
        self.assertEqual(group_lines(lines, "unstartable"), ["[ci] ci-metrics: measurement command could not start"])

    def test_a_line_without_the_prefix_is_sanitized_and_given_it(self):
        metrics = suites.collector()
        self.assertEqual(suites.quote(b"[ci] Ran 3 tests in 0.001s\n", metrics), "[ci] Ran 3 tests in 0.001s")
        self.assertEqual(suites.quote(b"::endgroup::\n", metrics), "[ci] ::endgroup::")
        self.assertEqual(suites.quote(b'  File "/Users/runner/work/metrics.py", line 3\n', metrics), '[ci]   File "[path]", line 3')
        self.assertEqual(suites.quote(b"see https://example.invalid/x?token=abcdefgh now\n", metrics), "[ci] see [url] now")
        self.assertEqual(suites.quote(b"x" * (metrics.MAX_LINE + 1), metrics), "[ci] [overlong diagnostic line omitted]")
        self.assertEqual(suites.quote(b"\xff\x1b[31mred\x1b[0m\r\n", metrics), "[ci] �red")

    def test_a_suite_that_runs_no_test_fails_the_step(self):
        status, lines, records, _ = drive([self.suite("passing"), self.suite("empty")])
        self.assertEqual(status, 1)
        self.assertEqual(records["passing"]["exit_code"], 0)
        self.assertIn("tooling-suites: FAILED: empty ran no test", lines)
        self.assertRegex("\n".join(lines), r"\ntooling-suites: empty: exit \d, \d+\.\d s, 0 tests\n")

    def test_an_interrupted_driver_ends_what_it_started_and_still_reports(self):
        with tempfile.TemporaryDirectory() as directory:
            started = Path(directory) / "started"
            # The half second before it says so is for the collector, which takes its signals a
            # few statements after it has started its command.
            waiting = ("import os, pathlib, sys, time; time.sleep(0.5); "
                       "pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(300)")
            program = textwrap.dedent("""
                import importlib.util, pathlib, sys
                spec = importlib.util.spec_from_file_location("driver", sys.argv[1])
                driver = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(driver)
                job = driver.Job("waiting", (sys.executable, "-c", sys.argv[3], sys.argv[4]), None)
                sys.exit(driver.run(pathlib.Path(sys.argv[2]), [job], None, cwd=sys.argv[2]))
            """)
            driver = subprocess.Popen([sys.executable, "-B", "-c", program, str(DRIVER), directory, waiting, str(started)],
                                      stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            try:
                deadline = time.monotonic() + 120
                while not (started.exists() and started.read_text(encoding="utf-8")):
                    self.assertIsNone(driver.poll(), "the driver ended before its process started")
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.02)
                driver.send_signal(signal.SIGTERM)
                output, _ = driver.communicate(timeout=120)
            finally:
                if driver.poll() is None:
                    driver.kill()
                    driver.wait()
                if started.exists() and started.read_text(encoding="utf-8"):
                    with contextlib.suppress(ProcessLookupError):
                        os.kill(int(started.read_text(encoding="utf-8")), signal.SIGKILL)
            # The collector reports its command's end by SIGTERM as 143; the driver waited for it and reported.
            self.assertEqual(driver.returncode, 1, output)
            self.assertIn("\ntooling-suites: FAILED: waiting exited 143\n", output)
            self.assertEqual(json.loads((Path(directory) / "waiting.json").read_text(encoding="utf-8"))["exit_code"], -15)


class PlanTests(unittest.TestCase):
    def test_the_step_is_six_processes_with_the_commands_the_four_steps_had(self):
        jobs, listing = suites.plan(Path("/records"))
        self.assertEqual([job.name for job in jobs],
                         ["agent-tooling-1-of-3", "agent-tooling-2-of-3", "agent-tooling-3-of-3",
                          "native-qa-tooling", "operator-tooling", "ci-tests"])
        discovery = ("-s", "scripts/agent_loop", "-t", "scripts", "-p", "test_*.py")
        for index, job in enumerate(jobs[:3]):
            self.assertEqual(job.command, (sys.executable, str(DRIVER), "group", "--index", str(index), "--of", "3",
                                           "--record", f"/records/group-{index + 1}.json", *discovery))
            self.assertEqual(job.record, Path(f"/records/group-{index + 1}.json"))
        self.assertEqual([job.command for job in jobs[3:]], [
            (sys.executable, "-m", "unittest", "discover", "-s", "scripts/native_qa", "-t", "scripts", "-p", "test_*.py"),
            (sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/operator", "-p", "test_*.py"),
            (sys.executable, "-m", "unittest", "discover", "-s", "scripts/ci/tests", "-p", "test_*.py")])
        self.assertEqual([job.record for job in jobs[3:]], [None, None, None])
        self.assertEqual(listing.command, (sys.executable, str(DRIVER), "ids", "--record", "/records/discovery.json", *discovery))
        # Every name is one the collector accepts, and the records sort by suite in its summary.
        metrics = suites.collector()
        for job in (*jobs, listing):
            self.assertTrue(metrics.NAME.fullmatch(job.name), job.name)
        # A start folder that is where the tests say it is: a renamed suite must not leave a group of nothing.
        for folder in ("scripts/agent_loop", "scripts/native_qa", "scripts/operator", "scripts/ci/tests"):
            self.assertTrue(any((ROOT / folder).glob("test_*.py")), folder)


class WorkflowTests(unittest.TestCase):
    def test_the_job_keeps_its_name_matrix_and_ten_minute_limit(self):
        job, _ = quality_job("agent-tooling")
        self.assertTrue(job.startswith(
            "    needs: changes\n    if: needs.changes.outputs.tooling == 'true'\n"
            "    name: Development tooling · ${{ matrix.os }}\n    runs-on: ${{ matrix.os }}\n    timeout-minutes: 10\n"
            "    strategy:\n      fail-fast: false\n      max-parallel: 2\n      matrix:\n"
            "        os: [macos-15, ubuntu-24.04]\n    steps:\n"), job[:400])
        self.assertEqual(re.findall(r"^ *timeout-minutes: *(.*)$", job, re.M), ["10"])

    def test_one_step_runs_the_suites_and_nothing_lets_a_failure_pass(self):
        job, steps = quality_job("agent-tooling")
        self.assertTrue(steps[0].startswith("uses: actions/checkout@"), steps[0])
        self.assertEqual([step.split("\n", 1)[0] for step in steps[1:]], [
            "name: Verify Python runtime",
            "name: Validate development guidance",
            "name: Test development, native-QA, operator and CI tooling side by side",
            "name: Check CI collector syntax and help",
            "name: Summarize CI measurements",
            "name: Preserve CI measurements and sanitized diagnostics"])
        # The whole step: its name and its command, with no condition, limit, retry or shell of its own.
        self.assertEqual(steps[3], "name: Test development, native-QA, operator and CI tooling side by side\n"
                                   f"        run: {STEP}")
        for word in ("continue-on-error", "retry", "|| true", "unittest discover"):
            self.assertNotIn(word, job, word)
        # Only the summary and the artifact run after a failure; nothing else is conditional.
        self.assertEqual(re.findall(r"^        if: (.*)$", job, re.M), ["always()", "always()"])
        self.assertEqual(job.count("scripts/ci/tooling_suites.py"), 1)


if __name__ == "__main__":
    unittest.main()
