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
listed here, inside a log group of its name. Every such line is printed behind the collector's
`[ci] ` prefix, on one line, and unable to spell a workflow command: control characters are
removed, the separators U+2028, U+2029 and U+0085 become a space, and the runner's older command
spelling `##[`, which it reads anywhere in a line, is broken up as `#\\x23[`. That is done to a
line the collector prefixed as much as to one it wrote about itself (which also goes through its
sanitizer here): what the collector starts runs as the same user and can write behind the prefix.
The driver's own lines are `::group::`, `::endgroup::` and those that start with
`tooling-suites: `; a test id in one of them is treated as suite output.

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
# Seconds this driver gives the step, counted from its own start. The job's limit of 10 minutes
# is unchanged and still has the last word: this one is stricter, never wider, and nothing is
# run again after it. The steps before this one took 8.3 to 29.5 s in 61 retained macOS runs and
# up to 31 s in one more, so 31 + 510 + 10 s of grace is 551 s, about 49 s inside the job's 600 s.
# A step cut here still prints what every process had written and names what it cut; a job cut by
# its own limit shows only the lines printed while it waited.
DEADLINE = 510.0
# Seconds between SIGTERM and SIGKILL for a session that is being ended.
GRACE = 10.0
PREFIX = "[ci] "
# Characters that end a line for some readers of a log, though not for this driver.
SEPARATORS = str.maketrans({"\N{LINE SEPARATOR}": " ", "\N{PARAGRAPH SEPARATOR}": " ", "\x85": " "})
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


def end_sessions(items, grace, clock):
    """End the processes that have no exit status yet; return those that were asked to end.

    SIGTERM goes to each one's session and SIGKILL, `grace` seconds later, to those still there.
    Both go through `signal_session` only, so to sessions this driver started, and only while
    `Popen` has not reaped the process: until then its number cannot belong to another one. The
    waiters are joined before this returns, so that nothing still writes to a capture.

    These are the collectors. A collector passes SIGTERM on to its command's session and waits
    for it; a command that handles or ignores the signal is still there when its collector is
    killed, and no session of its own is known here to signal.
    """
    asked = [item for item in items if item.child.returncode is None]
    for item in asked:
        signal_session(item.child, signal.SIGTERM)
    patience = clock() + grace
    for item in asked:
        item.waiter.join(timeout=max(0.0, patience - clock()))
    for item in asked:
        if item.child.returncode is None:
            signal_session(item.child, signal.SIGKILL)
    for item in items:
        # Nothing outlives SIGKILL; the limit only keeps a surprise from holding the step for good.
        item.waiter.join(timeout=max(grace, 1.0))
    return asked


class Running:
    """One started process: its captured output, and its exit status and seconds once it has ended."""

    def __init__(self, name, argv, cwd, clock):
        self.name, self.exit, self.seconds, self.cut = name, None, None, False
        # Unnamed, and held by the collector alone. What the collector starts still runs as the
        # same user and can reach the collector's descriptors, so `quote` trusts no line in it.
        self.capture = tempfile.TemporaryFile()
        self.clock, self.started = clock, clock()
        # Its own session, as `metrics.py measure` starts its command.
        self.child = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=self.capture,
                                      stderr=subprocess.STDOUT, start_new_session=True)
        self.waiter = threading.Thread(target=self.reap, daemon=True)
        self.waiter.start()

    def reap(self):
        status = self.child.wait()
        self.seconds = self.clock() - self.started
        self.exit = status

    def elapsed(self):
        return self.clock() - self.started if self.seconds is None else self.seconds

    def state(self):
        if self.exit is None:
            return f"{self.name} running ({os.fstat(self.capture.fileno()).st_size} bytes of output)"
        return f"{self.name} exit {self.exit} after {self.seconds:.1f} s"


def side_by_side(commands, cwd, beat=HEARTBEAT, until=None, grace=GRACE, clock=time.monotonic):
    """Start every (name, command) at once and wait until all of them have ended, or until `until`.

    No process is ended for another's failure: one that fails leaves the others running. A
    SIGTERM or SIGINT to this driver is passed to the sessions it started and the wait goes on,
    as `metrics.py measure` treats its command. At `until` on `clock`, what still runs is ended
    (`end_sessions`) and marked as cut. Any other way out of here ends those sessions first.
    Returns the processes and the signals that were passed on.
    """
    running, passed = [], []

    def forward(signum, _frame):
        passed.append(signum)
        for item in running:
            if item.child.returncode is None:
                signal_session(item.child, signum)

    previous = {signum: signal.signal(signum, forward) for signum in (signal.SIGTERM, signal.SIGINT)}
    try:
        origin = clock()
        for name, argv in commands:
            running.append(Running(name, argv, cwd, clock))
        beat_at = origin + beat
        while alive := [item for item in running if item.waiter.is_alive()]:
            now = clock()
            if until is not None and now >= until:
                late = [item for item in alive if item.child.returncode is None]
                if late:
                    # Said before the wait for them, so that the log has it whatever happens next.
                    say("the step's deadline has passed: ending " + ", ".join(item.name for item in late))
                for item in end_sessions(alive, grace, clock):
                    item.cut = True
                break
            alive[0].waiter.join(timeout=max(0.0, min(1.0, beat_at - now, *(() if until is None else (until - now,)))))
            if clock() >= beat_at:
                beat_at += beat
                # The output is printed at the end, so say meanwhile that the step is alive.
                say(f"+{clock() - origin:.0f} s: " + "; ".join(item.state() for item in running))
    except BaseException:
        end_sessions(running, grace, clock)
        raise
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    return running, passed

def harmless(text):
    """Text that stays on its line and spells no workflow command.

    Control characters go, and the separators that end a line for some readers become a space.
    The runner reads its older command spelling `##[` anywhere in a line, so that is broken up
    last, exactly as `shown` in `.github/actions/setup-rust/cache.py` breaks it.
    """
    kept = "".join(character for character in text.translate(SEPARATORS)
                   if character == "\t" or not (character < " " or "\x7f" <= character <= "\x9f"))
    return kept.replace("##[", "#\\x23[")


def named(identifier, metrics):
    """A test id as the driver may print it in a line of its own: it is the tests' text, not the driver's."""
    return harmless(" ".join(metrics.sanitize(identifier).split()))


def quote(raw, metrics):
    """One captured line as it may be printed: behind the collector's prefix, on one line and harmless."""
    text = raw.decode("utf-8", errors="replace").rstrip("\n")
    if text.startswith(PREFIX):
        # The collector's print of a line it sanitized, as far as can be told from here: the
        # prefix is no proof of that, so the line is made harmless like any other.
        return PREFIX + harmless(metrics.ANSI.sub("", text[len(PREFIX):]))
    # The collector's own words (a refusal, a traceback): its rule for a long line, then its sanitizer.
    if len(raw) > metrics.MAX_LINE:
        return PREFIX + "[overlong diagnostic line omitted]"
    return PREFIX + harmless(metrics.sanitize(text))


def replay(item, metrics):
    """Print one process's output whole in a log group; return the test count of unittest's last summary in it."""
    ran = None
    print(f"::group::{item.name}")
    if item.waiter.is_alive():
        # The capture shares its file position with the collector: reading it under a live
        # collector would move the collector's writes. Not reached unless SIGKILL failed.
        say(f"{item.name} could not be ended, so its output is not printed")
    else:
        item.capture.seek(0)
        # Split at line feeds only, as the collector wrote them; `quote` takes every other separator
        # out of the line, so none of them can start a line that would stand in the log without the prefix.
        for raw in item.capture:
            line = quote(raw, metrics)
            match = RAN.match(line)
            if match:
                ran = int(match.group(1))
            print(line)
        item.capture.close()
    print("::endgroup::", flush=True)
    return ran


def measured(job, directory):
    """One job's command under the collector, which keeps its sanitized log and timing record in `directory`."""
    return job.name, [sys.executable, str(METRICS), "measure", "--name", job.name, "--directory", str(directory),
                      "--", *job.command]


def run(directory, jobs, listing, cwd=ROOT, beat=HEARTBEAT, deadline=DEADLINE, grace=GRACE, clock=time.monotonic):
    metrics = collector()
    directory.mkdir(parents=True, exist_ok=True)
    say(f"starting {len(jobs)} processes at once: {', '.join(job.name for job in jobs)}")
    begun = clock()
    # One deadline for everything this step waits for, the proof's discovery included.
    until = begun + deadline
    running, passed = side_by_side([measured(job, directory) for job in jobs], cwd, beat, until, grace, clock)
    whole = clock() - begun
    counts = [replay(item, metrics) for item in running]
    groups = {job.name: read_record(job.record) for job in jobs if job.record is not None}
    discovered, problems, attempted = None, [], False
    if groups and not passed and not any(item.cut for item in running):
        # The driver's own discovery, under the collector too; its record stays out of the measurements.
        (found,), passed = side_by_side([measured(listing, listing.record.parent)], cwd, beat, until, grace, clock)
        running.append(found)
        counts.append(None)
        if found.exit != 0:
            replay(found, metrics)
        found.capture.close()
        if found.exit == 0:
            attempted = True
            discovered = read_record(listing.record)
            problems = prove(discovered, groups)
    for item, count in zip(running[:len(jobs)], counts):
        say(f"{item.name}: {'cut at the deadline, ' if item.cut else ''}exit {item.exit}, {item.elapsed():.1f} s, "
            f"{'no' if count is None else count} tests")
    say(f"{len(jobs)} processes side by side took {whole:.1f} s")
    failures = []
    for item in running:
        if item.cut:
            # The summary may have no row for it: a collector that had to be killed leaves no record.
            failures.append(f"{item.name} was cut at the deadline of {deadline:.0f} s, after {item.elapsed():.1f} s")
        elif item.exit != 0:
            failures.append(f"{item.name} exited {item.exit}")
    failures += [f"{item.name} ran no test" for item, count in zip(running[:len(jobs)], counts) if not count and not item.cut]
    failures += [f"the driver was sent {signal.Signals(signum).name} and passed it on" for signum in dict.fromkeys(passed)]
    if groups and not attempted:
        say("the proof that the groups hold every test once was not attempted: the step had already failed")
    elif groups and not problems:
        # Said whether or not a process failed: a failed test is then known not to be a lost one.
        shares = " + ".join(str(len(held)) for held in groups.values())
        say(f"the {len(groups)} agent-loop groups hold the {len(discovered)} discovered tests, each once ({shares})")
    for reason, identifier in problems:
        # A test id is sanitized and made harmless like suite output, and stands last on its line.
        say(f"PROOF FAILED: {reason}" + ("" if identifier is None else ": " + named(identifier, metrics)))
    for failure in failures:
        say(f"FAILED: {failure}")
    if failures or problems:
        return 1
    say(f"all {len(jobs)} processes passed")
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
