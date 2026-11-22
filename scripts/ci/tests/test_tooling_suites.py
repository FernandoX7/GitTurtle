"""The tooling job's side-by-side step: the driver's output, deadlines, signals and workflow shape."""

import contextlib
import importlib.util
import io
import itertools
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import types
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[3]
DRIVER = ROOT / "scripts/ci/tooling_suites.py"
SPEC = importlib.util.spec_from_file_location("ci_tooling_suites", DRIVER)
suites = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(suites)

STEP = 'python3 scripts/ci/tooling_suites.py run --directory "$RUNNER_TEMP/ci-metrics"'
FIXTURES = {
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
    # Waits for the failing suite's record and then runs half a second more: ten of the wake-ups
    # of the driver that runs it, in which a driver that ended the others at a failure would end it.
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
                time.sleep(0.5)
                Path(os.environ["TOOLING_SUITES_TEST_MARK"]).write_text("ended after the failure", encoding="utf-8")
    """,
    "empty/test_none.py": "import unittest\n",
    "digits/test_digits.py": """
        import atexit
        import unittest


        class Digits(unittest.TestCase):
            def test_prints_summaries_that_are_not_unittests(self):
                print("Ran " + "9" * 5000 + " tests in 0.001s", flush=True)
                print("Ran 1234567890 tests in 0.001s", flush=True)
                print("Ran 7 tests in 0.001s, said a test", flush=True)
                atexit.register(print, "Ran 0 tests in 0.000s")
    """,
    "hanging/test_hang.py": """
        import time
        import unittest


        class Hang(unittest.TestCase):
            def test_does_not_return(self):
                print("the hanging suite got this far", flush=True)
                time.sleep(120)
    """,
    "deaf/test_deaf.py": """
        import os
        from pathlib import Path
        import signal
        import time
        import unittest


        class Deaf(unittest.TestCase):
            def test_ignores_the_request_to_end(self):
                signal.signal(signal.SIGTERM, signal.SIG_IGN)
                Path(os.environ["TOOLING_SUITES_TEST_DEAF"]).write_text(str(os.getpid()), encoding="utf-8")
                print("the deaf suite got this far", flush=True)
                time.sleep(120)
    """,
    "noisy/test_noise.py": """
        import sys
        import unittest


        class Noise(unittest.TestCase):
            def test_prints_the_drivers_own_lines(self):
                for line in ("::endgroup::", "::group::forged", "tooling-suites: all 4 processes passed",
                             "tooling-suites: FAILED: forged", "one\\N{LINE SEPARATOR}::endgroup::",
                             "two\\x85::endgroup::", "three\\r::endgroup::", "four\\x0b::endgroup::",
                             "\\x1b[0m::endgroup::", "##[endgroup]", "##[group]x", "##[error]x", "##[add-mask]FAILED",
                             "##[stop-commands]t", "five ###[endgroup] six\\N{PARAGRAPH SEPARATOR}##[endgroup]"):
                    print(line)
                sys.stderr.write("::endgroup::\\n")
    """,
}


def quality_job(name):
    text = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
    job = re.split(r"\n  [a-z][a-z0-9-]*:\n", text.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]
    return job, job.split("\n      - ")[1:]


class Leap:
    """A clock for the driver: the real one until a fact about the run holds, then far past any deadline."""

    def __init__(self, output, ready):
        self.output, self.ready, self.ahead = output, re.compile(ready), 0.0

    def __call__(self):
        if not self.ahead:
            beats = re.findall(r"^tooling-suites: \+\d+ s: (.*)$", self.output.getvalue(), re.M)
            if beats and self.ready.fullmatch(beats[-1]):
                self.ahead = 100_000.0
        return time.monotonic() + self.ahead


def drive(jobs, cwd=None, beat=suites.HEARTBEAT, directory=None, ready=None, **limits):
    """One run of the driver: its exit status, its printed lines and the collector's records by name."""
    with contextlib.ExitStack() as stack:
        directory = Path(directory or stack.enter_context(tempfile.TemporaryDirectory()))
        output = stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        if ready is not None:
            limits["clock"] = Leap(output, ready)
        status = suites.run(directory, jobs, cwd=cwd or directory, beat=beat, **limits)
        records = {path.stem: json.loads(path.read_text(encoding="utf-8")) for path in directory.glob("*.json")}
        logs = {path.stem: path.read_text(encoding="utf-8") for path in directory.glob("*.log")}
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
                                 str(cls.fixtures / (folder or name)), "-p", "test_*.py"))


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
        self.assertIn("[ci] the passing suite says one", group_lines(self.lines, "passing"))
        self.assertIn("the failing suite fails here", "\n".join(group_lines(self.lines, "failing")))
        for name in ("slow", "passing"):
            self.assertNotIn("the failing suite fails here", "\n".join(group_lines(self.lines, name)))
        first_group = self.lines.index("::group::slow")
        for beat in (index for index, line in enumerate(self.lines) if re.match(r"tooling-suites: \+\d+ s: ", line)):
            self.assertLess(beat, first_group)
            self.assertRegex(self.lines[beat], r"^tooling-suites: \+\d+ s: slow (running \(\d+ bytes of output\)|exit 0 after )")

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
        status, lines, _, _ = drive([self.suite("noisy"), suites.Job("unstartable", ("/nonexistent/tooling-suites-program",))])
        self.assertEqual(status, 1)
        self.assertEqual([re.sub(r"\d+\.\d s", "N s", line) for line in lines if not line.startswith("[ci] ")],
                         ["tooling-suites: starting 2 processes at once: noisy, unstartable",
                          "::group::noisy", "::endgroup::", "::group::unstartable", "::endgroup::",
                          "tooling-suites: noisy: exit 0, N s, 1 tests",
                          "tooling-suites: unstartable: exit 2, N s, no tests",
                          "tooling-suites: 2 processes side by side took N s",
                          "tooling-suites: FAILED: unstartable exited 2",
                          "tooling-suites: FAILED: unstartable ran no test", ""])
        noisy = group_lines(lines, "noisy")
        for line in ("[ci] ::endgroup::", "[ci] ::group::forged", "[ci] tooling-suites: all 4 processes passed",
                     "[ci] tooling-suites: FAILED: forged", "[ci] one ::endgroup::", "[ci] two ::endgroup::",
                     "[ci] three::endgroup::", "[ci] four::endgroup::", "[ci] #\\x23[endgroup]", "[ci] #\\x23[group]x",
                     "[ci] #\\x23[error]x", "[ci] #\\x23[add-mask]FAILED", "[ci] #\\x23[stop-commands]t",
                     "[ci] five ##\\x23[endgroup] six #\\x23[endgroup]"):
            self.assertIn(line, noisy)
        self.assertEqual(noisy.count("[ci] ::endgroup::"), 3)
        for line in lines:
            self.assertNotIn("##[", line)
            self.assertFalse(set(line) & {"\N{LINE SEPARATOR}", "\N{PARAGRAPH SEPARATOR}", "\x85", "\r", "\x1b"}, line)
        self.assertEqual(group_lines(lines, "unstartable"), ["[ci] ci-metrics: measurement command could not start"])

    def test_a_line_without_the_prefix_is_sanitized_and_given_it(self):
        metrics = suites.collector()
        self.assertEqual(suites.quote(b"[ci] Ran 3 tests in 0.001s\n", metrics), "[ci] Ran 3 tests in 0.001s")
        self.assertEqual(suites.quote(b"::endgroup::\n", metrics), "[ci] ::endgroup::")
        self.assertEqual(suites.quote(b'  File "/Users/runner/work/metrics.py", line 3\n', metrics), '[ci]   File "[path]", line 3')
        self.assertEqual(suites.quote(b"see https://example.invalid/x?token=abcdefgh now\n", metrics), "[ci] see [url] now")
        self.assertEqual(suites.quote(b"x" * (metrics.MAX_LINE + 1), metrics), "[ci] [overlong diagnostic line omitted]")
        self.assertEqual(suites.quote(b"\xff\x1b[31mred\x1b[0m\r\n", metrics), "[ci] \N{REPLACEMENT CHARACTER}red")
        for raw, printed in ((b"##[endgroup]", "#\\x23[endgroup]"), (b"x ##[error]y", "x #\\x23[error]y"),
                             (b"a\xc2\x85##[add-mask]FAILED", "a #\\x23[add-mask]FAILED"),
                             (b"a\xe2\x80\xa8b\xe2\x80\xa9c\xc2\x9bd", "a b cd")):
            self.assertEqual(suites.quote(raw + b"\n", metrics), "[ci] " + printed, raw)

    def test_a_line_with_the_prefix_is_not_trusted_either(self):
        metrics = suites.collector()
        for raw, printed in ((b"##[endgroup]", "#\\x23[endgroup]"), (b"##[group]x", "#\\x23[group]x"),
                             (b"##[error]x", "#\\x23[error]x"), (b"##[add-mask]FAILED", "#\\x23[add-mask]FAILED"),
                             (b"##[stop-commands]t", "#\\x23[stop-commands]t"), (b"x ###[endgroup]", "x ##\\x23[endgroup]"),
                             (b"#\x00#[endgroup]", "#\\x23[endgroup]"), (b"#\x1b[0m#[endgroup]", "#\\x23[endgroup]"),
                             (b"a\x1b[31mb\x1b[0m\x1bc\rd\x7fe\x0bf", "abcdef"),
                             (b"one\xe2\x80\xa8::endgroup::", "one ::endgroup::"),
                             (b"two\xe2\x80\xa9::endgroup::\xc2\x85##[endgroup]\xc2\x9b0m", "two ::endgroup:: #\\x23[endgroup]0m"),
                             (b"tab\tstays, \xc3\xa9 too", "tab\tstays, \N{LATIN SMALL LETTER E WITH ACUTE} too")):
            self.assertEqual(suites.quote(b"[ci] " + raw + b"\n", metrics), "[ci] " + printed, raw)

    def test_the_collectors_rule_gives_every_byte_the_drivers_own_gave(self):
        metrics = suites.collector()

        def before(text):
            separators = str.maketrans({"\N{LINE SEPARATOR}": " ", "\N{PARAGRAPH SEPARATOR}": " ", "\x85": " "})
            kept = "".join(character for character in text.translate(separators)
                           if character == "\t" or not (character < " " or "\x7f" <= character <= "\x9f"))
            return kept.replace("##[", "#\\x23[")

        texts = [form.format(chr(code)) for code in range(0x3000) for form in ("a{}b", "#{}#[", "##{}[")]
        texts += ["".join(characters) for length in range(1, 6)
                  for characters in itertools.product("#[\\a\t\n\x00\x85\x9b\N{LINE SEPARATOR}", repeat=length)]
        for text in texts:
            if metrics.harmless(text) != before(text):
                self.fail(ascii(text))

    def test_a_line_the_collector_printed_is_quoted_as_it_stands(self):
        metrics = suites.collector()
        cases = (("##[error]x", "#\\x23[error]x"), ("x ###[endgroup] ##[group]y", "x ##\\x23[endgroup] #\\x23[group]y"),
                 ("#\x00#[add-mask]FAILED", "#\\x23[add-mask]FAILED"), ("one\N{LINE SEPARATOR}::endgroup::", "one ::endgroup::"),
                 ("two\x85::endgroup::\x9b0m", "two ::endgroup::0m"), ("tab\tstays", "tab\tstays"),
                 ("Ran 3 tests in 0.001s", "Ran 3 tests in 0.001s"))
        program = f"import sys\nfor line in {[raw for raw, _ in cases]!r}:\n    sys.stdout.buffer.write(line.encode('utf-8') + b'\\n')\n"
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, "-B", str(suites.METRICS), "measure", "--name", "printed",
                                     "--directory", directory, "--", sys.executable, "-c", program],
                                    capture_output=True, check=False)
        self.assertEqual((result.returncode, result.stderr), (0, b""))
        printed = result.stdout.split(b"\n")[:-1]
        self.assertEqual(printed, [("[ci] " + line).encode("utf-8") for _, line in cases])
        for line in printed:
            self.assertEqual(suites.quote(line + b"\n", metrics).encode("utf-8"), line)

    def test_summary_lines_that_a_test_prints_neither_stop_the_replay_nor_pass_for_unittests(self):
        status, lines, _, _ = drive([self.suite("digits"), self.suite("passing")])
        self.assertEqual(status, 0, lines[-8:])
        digits = group_lines(lines, "digits")
        for line in ("[ci] Ran " + "9" * 5000 + " tests in 0.001s", "[ci] Ran 1234567890 tests in 0.001s",
                     "[ci] Ran 7 tests in 0.001s, said a test"):
            self.assertIn(line, digits)
        (own,) = [index for index, line in enumerate(digits) if re.fullmatch(r"\[ci\] Ran 1 test in \d+\.\d{3}s", line)]
        self.assertLess(own, digits.index("[ci] Ran 0 tests in 0.000s"))
        self.assertEqual([re.sub(r"\d+\.\d s", "N s", line) for line in lines[-5:]],
                         ["tooling-suites: digits: exit 0, N s, 1 tests", "tooling-suites: passing: exit 0, N s, 2 tests",
                          "tooling-suites: 2 processes side by side took N s", "tooling-suites: all 2 processes passed", ""])

    def test_only_unittests_whole_summary_line_with_a_bounded_count_is_a_count(self):
        for line, count in (("[ci] Ran 3 tests in 0.001s", "3"), ("[ci] Ran 1 test in 12.345s", "1"),
                            ("[ci] Ran 999999999 tests in 0.000s", "999999999")):
            self.assertEqual(suites.RAN.fullmatch(line).group(1), count)
        for line in ("[ci] Ran 3 tests in 0.001s and more", "[ci] it Ran 3 tests in 0.001s", "[ci] Ran 1234567890 tests in 0.001s",
                     "[ci] Ran 3 tests in 1s", "[ci] Ran  tests in 0.001s", "Ran 3 tests in 0.001s"):
            self.assertIsNone(suites.RAN.fullmatch(line), line)

    def test_output_that_cannot_be_printed_fails_the_step_and_stops_no_other_replay(self):
        quote = suites.quote

        def unprintable(raw, metrics):
            if b"the passing suite says one" in raw:
                raise ValueError("a line the replay cannot take")
            return quote(raw, metrics)

        with mock.patch.object(suites, "quote", unprintable):
            status, lines, _, _ = drive([self.suite("passing"), self.suite("failing")])
        self.assertEqual(status, 1)
        self.assertEqual([line for line in lines if line.startswith("::")],
                         ["::group::passing", "::endgroup::", "::group::failing", "::endgroup::"])
        self.assertIn("the failing suite fails here", "\n".join(group_lines(lines, "failing")))
        self.assertEqual([re.sub(r"\d+\.\d s", "N s", line) for line in lines[-6:]],
                         ["tooling-suites: passing: exit 0, N s, 2 tests", "tooling-suites: failing: exit 1, N s, 1 tests",
                          "tooling-suites: 2 processes side by side took N s",
                          "tooling-suites: FAILED: failing exited 1",
                          "tooling-suites: FAILED: the output of passing could not be printed whole (ValueError)", ""])

    def test_a_suite_that_runs_no_test_fails_the_step(self):
        status, lines, records, _ = drive([self.suite("passing"), self.suite("empty")])
        self.assertEqual(status, 1)
        self.assertEqual(records["passing"]["exit_code"], 0)
        self.assertIn("tooling-suites: FAILED: empty ran no test", lines)
        self.assertRegex("\n".join(lines), r"\ntooling-suites: empty: exit \d, \d+\.\d s, 0 tests\n")

    def test_an_interrupted_driver_ends_what_it_started_and_reports(self):
        directory = Path(self.enterContext(tempfile.TemporaryDirectory()))
        started = directory / "started"
        waiting = ("import os, pathlib, sys, time; pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); "
                   "print('the command is waiting', flush=True); time.sleep(120)")
        program = textwrap.dedent("""
            import importlib.util, pathlib, sys
            spec = importlib.util.spec_from_file_location("driver", sys.argv[1])
            driver = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(driver)
            directory = pathlib.Path(sys.argv[2])
            jobs = [
                driver.Job("passing", (sys.executable, "-B", "-m", "unittest", "discover", "-s", sys.argv[5], "-p", "test_*.py")),
                driver.Job("waiting", (sys.executable, "-c", sys.argv[3], sys.argv[4])),
            ]
            sys.exit(driver.run(directory / "metrics", jobs, cwd=directory, beat=0.05))
        """)
        driver = subprocess.Popen(
            [sys.executable, "-B", "-c", program, str(DRIVER), str(directory), waiting, str(started), str(self.fixtures / "passing")],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        watchdog = threading.Timer(120, driver.kill)
        watchdog.start()
        seen = []
        try:
            for line in driver.stdout:
                seen.append(line)
                if re.search(r"; waiting running \([1-9]\d* bytes of output\)$", line.rstrip("\n")):
                    break
            else:
                self.fail("the driver ended before its command's output was in the capture:\n" + "".join(seen))
            driver.send_signal(signal.SIGTERM)
            seen.append(driver.stdout.read())
            driver.wait(timeout=120)
        finally:
            watchdog.cancel()
            if driver.poll() is None:
                driver.kill()
                driver.wait()
            driver.stdout.close()
            with contextlib.suppress(OSError, ValueError):
                os.kill(int(started.read_text(encoding="utf-8")), signal.SIGKILL)
        output = "".join(seen)
        lines = output.split("\n")
        self.assertEqual(driver.returncode, 1, output)
        self.assertIn("[ci] the command is waiting", lines)
        self.assertIn("tooling-suites: FAILED: waiting exited 143", lines)
        self.assertIn("tooling-suites: FAILED: the driver was sent SIGTERM and passed it on", lines)
        self.assertEqual(json.loads((directory / "metrics" / "waiting.json").read_text(encoding="utf-8"))["exit_code"], -15)


class DeadlineTests(Fixtures):
    """The driver's own deadline. Each run reaches it on the driver's clock, at a state its heartbeat has shown."""

    @staticmethod
    def after(lines, name):
        """The driver's last lines, after the named group, with every number of seconds as N."""
        return [re.sub(r"\d+\.\d s", "N s", line) for line in lines[lines.index("::endgroup::", lines.index(f"::group::{name}")) + 1:]]

    def test_a_process_that_hangs_is_cut_replayed_and_named(self):
        jobs = [self.suite("passing"), self.suite("hanging")]
        status, lines, measured, _ = drive(
            jobs, beat=0.05, deadline=3600, grace=30,
            ready=r"passing exit 0 after \S+ s; hanging running \([1-9]\d* bytes of output\)")
        self.assertEqual(status, 1, lines)
        self.assertIn("tooling-suites: the step's deadline has passed: ending hanging", lines)
        self.assertEqual(group_lines(lines, "hanging"), ["[ci] the hanging suite got this far"])
        self.assertIn("[ci] the passing suite says one", group_lines(lines, "passing"))
        self.assertEqual(self.after(lines, "hanging"), [
            "tooling-suites: passing: exit 0, N s, 2 tests",
            "tooling-suites: hanging: cut at the deadline, exit 143, N s, no tests",
            "tooling-suites: 2 processes side by side took N s",
            "tooling-suites: FAILED: hanging was cut at the deadline of 3600 s, after N s", ""])
        self.assertEqual(measured["hanging"]["exit_code"], -15)

    def test_a_process_that_ignores_the_request_is_killed_after_the_grace_and_still_replayed(self):
        mark = Path(self.enterContext(tempfile.TemporaryDirectory())) / "deaf"
        self.enterContext(mock.patch.dict(os.environ, {"TOOLING_SUITES_TEST_DEAF": str(mark)}))

        def end_the_suite():
            with contextlib.suppress(OSError, ValueError):
                os.kill(int(mark.read_text(encoding="utf-8")), signal.SIGKILL)

        self.addCleanup(end_the_suite)
        status, lines, measured, _ = drive([self.suite("passing"), self.suite("deaf")], beat=0.05, deadline=3600, grace=0.2,
                                           ready=r"passing exit 0 after \S+ s; deaf running \([1-9]\d* bytes of output\)")
        self.assertEqual(status, 1, lines)
        self.assertEqual(group_lines(lines, "deaf"), ["[ci] the deaf suite got this far"])
        self.assertIn("[ci] the passing suite says one", group_lines(lines, "passing"))
        self.assertEqual(self.after(lines, "deaf"), [
            "tooling-suites: passing: exit 0, N s, 2 tests",
            "tooling-suites: deaf: cut at the deadline, exit -9, N s, no tests",
            "tooling-suites: 2 processes side by side took N s",
            "tooling-suites: FAILED: deaf was cut at the deadline of 3600 s, after N s", ""])
        self.assertEqual(sorted(measured), ["passing"])
        os.kill(int(mark.read_text(encoding="utf-8")), 0)

    def test_output_is_not_read_while_its_collector_is_still_there(self):
        class Untouched:
            def __getattr__(self, name):
                raise AssertionError(f"the capture was used: {name}")

        item = types.SimpleNamespace(name="stays", capture=Untouched(), waiter=types.SimpleNamespace(is_alive=lambda: True))
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(suites.replay(item, suites.collector()), (None, None))
        self.assertEqual(output.getvalue().split("\n"),
                         ["::group::stays", "tooling-suites: stays could not be ended, so its output is not printed", "::endgroup::", ""])


class PlanTests(unittest.TestCase):
    def test_the_step_runs_the_four_tooling_suites_side_by_side(self):
        jobs = suites.plan()
        self.assertEqual([job.name for job in jobs],
                         ["dev-tooling", "gate-tooling", "native-qa-tooling", "ci-tests"])
        self.assertEqual([job.command for job in jobs], [
            (sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/dev", "-t", "scripts", "-p", "test_*.py"),
            (sys.executable, "-B", "-m", "unittest", "scripts/test_gate.py", "scripts/test_claude_rm_hook.py"),
            (sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/native_qa", "-t", "scripts", "-p", "test_*.py"),
            (sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/ci/tests", "-p", "test_*.py")])
        metrics = suites.collector()
        for job in jobs:
            self.assertTrue(metrics.NAME.fullmatch(job.name), job.name)
        for folder in ("scripts/dev", "scripts/native_qa", "scripts/ci/tests"):
            self.assertTrue(any((ROOT / folder).glob("test_*.py")), folder)
        for path in ("scripts/test_gate.py", "scripts/test_claude_rm_hook.py"):
            self.assertTrue((ROOT / path).is_file(), path)


class WorkflowTests(unittest.TestCase):
    def test_the_job_keeps_its_name_matrix_and_ten_minute_limit(self):
        job, _ = quality_job("agent-tooling")
        self.assertTrue(job.startswith(
            "    needs: changes\n    if: needs.changes.outputs.tooling == 'true'\n"
            "    name: Development tooling · ${{ matrix.os }}\n    runs-on: ${{ matrix.os }}\n    timeout-minutes: 10\n"
            "    strategy:\n      fail-fast: false\n      max-parallel: 2\n      matrix:\n"
            "        os: [macos-15, ubuntu-24.04]\n    steps:\n"), job[:400])
        self.assertEqual(re.findall(r"^ *timeout-minutes: *(.*)$", job, re.M), ["10"])
        self.assertEqual((suites.DEADLINE, suites.GRACE), (510.0, 10.0))
        self.assertLessEqual(31 + suites.DEADLINE + suites.GRACE, 10 * 60 - 45)

    def test_one_step_runs_the_suites_and_nothing_lets_a_failure_pass(self):
        job, steps = quality_job("agent-tooling")
        self.assertTrue(steps[0].startswith("uses: actions/checkout@"), steps[0])
        self.assertEqual([step.split("\n", 1)[0] for step in steps[1:]], [
            "name: Verify Python runtime",
            "name: Validate development guidance",
            "name: Validate documentation sync",
            "name: Test development, gate, native-QA and CI tooling side by side",
            "name: Check CI collector syntax and help",
            "name: Summarize CI measurements",
            "name: Preserve CI measurements and sanitized diagnostics"])
        self.assertEqual(
            steps[4],
            "name: Test development, gate, native-QA and CI tooling side by side\n"
            "        run: |\n"
            '          if [ "${{ runner.os }}" = "Linux" ]; then\n'
            '            python3 scripts/ci/tooling_suites.py run --skip-ci-tests --directory "$RUNNER_TEMP/ci-metrics"\n'
            "          else\n"
            f"            {STEP}\n"
            "          fi",
        )
        for word in ("continue-on-error", "retry", "|| true", "unittest discover"):
            self.assertNotIn(word, job, word)
        self.assertEqual(re.findall(r"^        if: (.*)$", job, re.M), ["always()", "always()"])
        self.assertEqual(job.count("scripts/ci/tooling_suites.py"), 2)


if __name__ == "__main__":
    unittest.main()
