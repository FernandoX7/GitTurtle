#!/usr/bin/env python3
"""Run Quality's development-tooling suites side by side (standard library only; macOS and Linux).

  python3 scripts/ci/tooling_suites.py run --directory <measurements>

`run` is the tooling job's one test step. It starts six processes at once, each through
`scripts/ci/metrics.py measure` and in a session of its own: the agent-loop suite as three groups,
then the native-QA, operator and CI-helper suites with the commands they always had. All six run
to their end whatever the others do, nothing is run twice, and the step fails when one of them
fails, ran no test, or the groups do not hold every agent-loop test exactly once.

The agent-loop tests are dealt, not listed: each group discovers the suite as
`python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'` does, sorts the
tests by id and keeps every third one from its own place, in that order. A test of one class
therefore stays beside its neighbours and the class and module fixtures still run around them, and
a new test or module is dealt without a list to edit. Each group records the ids it hands to
unittest; when the six have ended, a discovery of the driver's own must find exactly those ids,
each in one group, or the step fails and names every id that differs.

A process's output is captured and printed whole once all six have ended, in the order they are
listed here, inside a log group of its name. Every such line carries the collector's `[ci] `
prefix: the collector sanitized it and put the prefix there, or (for a line the collector wrote
about itself) this driver passes it through the collector's sanitizer and adds it. The driver's
own lines are `::group::`, `::endgroup::` and those that start with `tooling-suites: `, so a line
of suite output cannot open or close a group, nor pass for a line of the driver.

`group` and `ids` are the processes `run` starts for the agent-loop suite, with the discovery
arguments of `python3 -m unittest discover`. `group` also runs one group again as the job ran it:

  python3 scripts/ci/tooling_suites.py group --index 1 --of 3 --record <file> \\
    -s scripts/agent_loop -t scripts -p 'test_*.py'
"""

import argparse
import collections
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest

HERE = Path(__file__).resolve()
ROOT = HERE.parents[2]
METRICS = ROOT / "scripts/ci/metrics.py"
# The agent-loop suite's discovery, as its own step always ran it.
AGENT_LOOP = ("-s", "scripts/agent_loop", "-t", "scripts", "-p", "test_*.py")
GROUPS = 3
# The other suites: the measurement's name and the interpreter arguments its step always had.
SUITES = (
    ("native-qa-tooling", ("-m", "unittest", "discover", "-s", "scripts/native_qa", "-t", "scripts", "-p", "test_*.py")),
    ("operator-tooling", ("-B", "-m", "unittest", "discover", "-s", "scripts/operator", "-p", "test_*.py")),
    ("ci-tests", ("-m", "unittest", "discover", "-s", "scripts/ci/tests", "-p", "test_*.py")),
)
HEARTBEAT = 30.0
GRACE = 5.0
PREFIX = "[ci] "
# unittest's own last word on a run, as the collector prints it.
RAN = re.compile(r"\[ci\] Ran (\d+) tests? in \d")

# One process of the step: its measurement name, its command, and for an agent-loop group the
# file it records its test ids in.
Job = collections.namedtuple("Job", "name command record")


def say(text):
    print(f"tooling-suites: {text}", flush=True)


# The agent-loop groups.

def deal(items, index, count, key=None):
    """The share of group `index` out of `count`: every `count`-th item of the sorted ones, in that order."""
    return sorted(items, key=key)[index::count]


def flatten(suite):
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from flatten(item)
        else:
            yield item


def write_record(path, ids):
    partial = f"{path}.partial"
    with open(partial, "w", encoding="utf-8") as stream:
        json.dump({"ids": ids}, stream)
    os.replace(partial, path)


def read_record(path):
    """The test ids one process recorded, or None when it left no readable record."""
    try:
        ids = json.loads(Path(path).read_text(encoding="utf-8"))["ids"]
    except (OSError, ValueError, KeyError, TypeError):
        return None
    return ids if isinstance(ids, list) and all(isinstance(item, str) for item in ids) else None


class Dealer(unittest.TestLoader):
    """Discovery that keeps one group's share of the tests and records their ids."""

    def __init__(self, index, count, record):
        super().__init__()
        self.index, self.count, self.record = index, count, record

    def discover(self, start_dir, pattern="test*.py", top_level_dir=None):
        found = flatten(super().discover(start_dir, pattern, top_level_dir))
        kept = deal(found, self.index, self.count, key=lambda test: test.id())
        write_record(self.record, [test.id() for test in kept])
        # One flat suite in sorted order: unittest sets a class and a module up when it reaches
        # their first test here and tears them down after their last.
        return unittest.TestSuite(kept)


def as_unittest(args):
    """Give this process the module path and arguments of `python3 -m unittest discover …`."""
    arguments = ["discover", "-s", args.start, *(["-t", args.top] if args.top else []), "-p", args.pattern]
    # `-m` has the working directory first on the module path; a script has its own folder there.
    if sys.path and os.path.realpath(sys.path[0]) == str(HERE.parent):
        sys.path[0] = os.getcwd()
    sys.argv = [os.path.basename(sys.executable) + " -m unittest", *arguments]


def group(args):
    as_unittest(args)
    # unittest's own program: its runner, warnings, output and exit status, with only the loader changed.
    unittest.main(module=None, argv=sys.argv, testLoader=Dealer(args.index, args.of, args.record))


def ids(args):
    as_unittest(args)
    found = flatten(unittest.TestLoader().discover(args.start, args.pattern, args.top))
    write_record(args.record, sorted(test.id() for test in found))
    return 0


def prove(discovered, groups):
    """Why the groups do not hold every discovered test exactly once; empty when they do.

    `groups` maps a group's name to the ids it recorded, or to None when it left no record. Each
    answer is a reason and the test id it is about, or None when it is about no single test.
    """
    if discovered is None:
        return [("the driver's own discovery left no readable list of test ids", None)]
    problems = [(f"{name} left no readable record of its test ids", None) for name, held in groups.items() if held is None]
    if not discovered:
        problems.append(("the driver's own discovery found no test", None))
    problems += [("discovered twice", identifier)
                 for identifier, count in sorted(collections.Counter(discovered).items()) if count > 1]
    where = collections.defaultdict(list)
    for name, held in groups.items():
        for identifier in held or ():
            where[identifier].append(name)
    found = set(discovered)
    problems += [("discovered, in no group", identifier) for identifier in sorted(found - set(where))]
    problems += [(f"in {names[0]}, not discovered", identifier)
                 for identifier, names in sorted(where.items()) if identifier not in found]
    problems += [(f"more than once, in {' and '.join(names)}", identifier)
                 for identifier, names in sorted(where.items()) if len(names) > 1]
    return problems


# The processes.

def plan(records, discovery=AGENT_LOOP, count=GROUPS, suites=SUITES):
    """The step's processes in the order their output is printed, and the driver's own discovery."""
    jobs = []
    for index in range(count):
        record = records / f"group-{index + 1}.json"
        jobs.append(Job(f"agent-tooling-{index + 1}-of-{count}",
                        (sys.executable, str(HERE), "group", "--index", str(index), "--of", str(count),
                         "--record", str(record), *discovery), record))
    jobs += [Job(name, (sys.executable, *arguments), None) for name, arguments in suites]
    record = records / "discovery.json"
    return jobs, Job("agent-tooling-discovery",
                     (sys.executable, str(HERE), "ids", "--record", str(record), *discovery), record)


def collector():
    """`metrics.py`, loaded by its path into this process only and leaving no bytecode behind."""
    writes, sys.dont_write_bytecode = sys.dont_write_bytecode, True
    try:
        spec = importlib.util.spec_from_file_location("tooling_suites_collector", METRICS)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = writes
    return module


def signal_session(child, signum):
    """Signal the session this driver started for one process, and nothing else."""
    try:
        os.killpg(child.pid, signum)
    except (ProcessLookupError, PermissionError):  # gone already; macOS answers EPERM for a group of zombies
        pass


class Running:
    """One started process: its captured output, and its exit status and seconds once it has ended."""

    def __init__(self, name, argv, cwd):
        self.name, self.exit, self.seconds = name, None, None
        # Unnamed, so nothing but the process itself can write to it.
        self.capture = tempfile.TemporaryFile()
        started = time.monotonic()
        # Its own session, as `metrics.py measure` starts its command.
        self.child = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=self.capture,
                                      stderr=subprocess.STDOUT, start_new_session=True)
        self.waiter = threading.Thread(target=self.reap, args=(started,), daemon=True)
        self.waiter.start()

    def reap(self, started):
        status = self.child.wait()
        self.seconds = time.monotonic() - started
        self.exit = status

    def state(self):
        if self.exit is None:
            return f"{self.name} running ({os.fstat(self.capture.fileno()).st_size} bytes of output)"
        return f"{self.name} exit {self.exit} after {self.seconds:.1f} s"


def side_by_side(commands, cwd, beat=HEARTBEAT):
    """Start every (name, command) at once and wait until all of them have ended.

    Nothing is ended early: a process that fails leaves the others running. A SIGTERM or SIGINT to
    this driver is passed to the sessions it started and the wait goes on, as `metrics.py measure`
    treats its command; any other way out of here ends those sessions first.
    """
    running = []

    def forward(signum, _frame):
        for item in running:
            if item.exit is None:
                signal_session(item.child, signum)

    previous = {signum: signal.signal(signum, forward) for signum in (signal.SIGTERM, signal.SIGINT)}
    try:
        origin = time.monotonic()
        for name, argv in commands:
            running.append(Running(name, argv, cwd))
        beat_at = origin + beat
        while alive := [item for item in running if item.waiter.is_alive()]:
            alive[0].waiter.join(timeout=max(0.0, min(1.0, beat_at - time.monotonic())))
            if time.monotonic() >= beat_at:
                beat_at += beat
                # The output is printed at the end, so say meanwhile that the step is alive.
                say(f"+{time.monotonic() - origin:.0f} s: " + "; ".join(item.state() for item in running))
    except BaseException:
        forward(signal.SIGTERM, None)
        patience = time.monotonic() + GRACE
        for item in running:
            item.waiter.join(timeout=max(0.0, patience - time.monotonic()))
        forward(signal.SIGKILL, None)
        for item in running:
            item.waiter.join(timeout=GRACE)
        raise
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    return running


def quote(raw, metrics):
    """One captured line as it may be printed: always behind the collector's prefix, and sanitized."""
    text = raw.decode("utf-8", errors="replace").rstrip("\n")
    if text.startswith(PREFIX):
        return text  # the collector's print of a line it sanitized
    # The collector's own words (a refusal, a traceback): its rule for a long line, then its sanitizer.
    if len(raw) > metrics.MAX_LINE:
        return PREFIX + "[overlong diagnostic line omitted]"
    return PREFIX + metrics.sanitize(text).replace("\n", " ")


def replay(item, metrics):
    """Print one process's output whole in a log group; return the test count of unittest's last summary in it."""
    ran = None
    print(f"::group::{item.name}")
    item.capture.seek(0)
    # Split at line feeds only, as the collector wrote them: a separator such as U+2028 inside a
    # line must not start a new one, which would stand in the log without the prefix.
    for raw in item.capture:
        line = quote(raw, metrics)
        match = RAN.match(line)
        if match:
            ran = int(match.group(1))
        print(line)
    print("::endgroup::", flush=True)
    item.capture.close()
    return ran


def measured(job, directory):
    """One job's command under the collector, which keeps its sanitized log and timing record in `directory`."""
    return job.name, [sys.executable, str(METRICS), "measure", "--name", job.name, "--directory", str(directory),
                      "--", *job.command]


def run(directory, jobs, listing, cwd=ROOT, beat=HEARTBEAT):
    metrics = collector()
    directory.mkdir(parents=True, exist_ok=True)
    say(f"starting {len(jobs)} processes at once: {', '.join(job.name for job in jobs)}")
    begun = time.monotonic()
    running = side_by_side([measured(job, directory) for job in jobs], cwd, beat)
    whole = time.monotonic() - begun
    counts = [replay(item, metrics) for item in running]
    failures = [f"{item.name} exited {item.exit}" for item in running if item.exit != 0]
    failures += [f"{item.name} ran no test" for item, count in zip(running, counts) if not count]
    groups = {job.name: read_record(job.record) for job in jobs if job.record is not None}
    discovered, problems = None, []
    if groups:
        # The driver's own discovery, under the collector too; its record stays out of the measurements.
        (found,) = side_by_side([measured(listing, listing.record.parent)], cwd, beat)
        if found.exit != 0:
            failures.append(f"the driver's own discovery exited {found.exit}")
            replay(found, metrics)
        found.capture.close()
        discovered = read_record(listing.record)
        problems = prove(discovered, groups)
    for item, count in zip(running, counts):
        say(f"{item.name}: exit {item.exit}, {item.seconds:.1f} s, {'no' if count is None else count} tests")
    say(f"{len(running)} processes side by side took {whole:.1f} s")
    if groups and not problems:
        # Said whether or not a process failed: a failed test is then known not to be a lost one.
        shares = " + ".join(str(len(held)) for held in groups.values())
        say(f"the {len(groups)} agent-loop groups hold the {len(discovered)} discovered tests, each once ({shares})")
    for reason, identifier in problems:
        # A test id is the tests' text, not the driver's: sanitized, and last on its line.
        say(f"PROOF FAILED: {reason}" + ("" if identifier is None else ": " + " ".join(metrics.sanitize(identifier).split())))
    for failure in failures:
        say(f"FAILED: {failure}")
    if failures or problems:
        return 1
    say(f"all {len(running)} processes passed")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="mode", required=True)
    step = sub.add_parser("run", help="run the six processes at once and fail when any of them does")
    step.add_argument("--directory", required=True, help="where `metrics.py measure` keeps each process's log and timing record")
    for mode, text in (("group", "run one group of a discovered suite and record its test ids"),
                       ("ids", "record the test ids a discovery finds, without running them")):
        child = sub.add_parser(mode, help=text)
        child.add_argument("--record", required=True)
        child.add_argument("-s", dest="start", required=True)
        child.add_argument("-t", dest="top")
        child.add_argument("-p", dest="pattern", required=True)
        if mode == "group":
            child.add_argument("--index", type=int, required=True, help="this group's place, from 0")
            child.add_argument("--of", type=int, required=True, help="the number of groups")
    args = parser.parse_args(argv)
    if args.mode == "run":
        with tempfile.TemporaryDirectory(prefix="tooling-suites-") as records:
            return run(Path(args.directory).resolve(), *plan(Path(records)))
    if args.mode == "group":
        if not 0 <= args.index < args.of:
            parser.error("--index must be at least 0 and less than --of")
        return group(args)
    return ids(args)


if __name__ == "__main__":
    sys.exit(main())
