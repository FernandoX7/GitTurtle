#!/usr/bin/env python3
"""Run Quality's development-tooling suites side by side (standard library only; macOS and Linux).

  python3 scripts/ci/tooling_suites.py run --directory <measurements>

`run` is the tooling job's one test step. It starts the development-tooling suites side by side,
each through `scripts/ci/metrics.py measure` and in a session of its own (`dev-tooling`,
`gate-tooling`, `native-qa-tooling`, and optionally `ci-tests`). All processes run to their end
whatever the others do, nothing is run twice, and the step fails when any suite fails or runs no
test. Only the driver's own deadline (`DEADLINE`, behind the job's limit) ends a process: what it
cuts is named, every process's output is still printed, and the step fails.

A process's output is captured and printed whole once all suites have ended, in the order they are
listed here, inside a log group of its name. Every such line is printed behind the collector's
`[ci] ` prefix, on one line, and unable to spell a workflow command: control characters are
removed, the separators U+2028, U+2029 and U+0085 become a space, and the runner's older command
spelling `##[`, which it reads anywhere in a line, is broken up as `#\\x23[`. That is done to a
line the collector prefixed as much as to one it wrote about itself (which also goes through its
sanitizer here): what the collector starts runs as the same user and can write behind the prefix.
The driver's own lines are `::group::`, `::endgroup::` and those that start with
`tooling-suites: `.
"""

import argparse
import collections
import importlib.util
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time

HERE = Path(__file__).resolve()
ROOT = HERE.parents[2]
METRICS = ROOT / "scripts/ci/metrics.py"
# Tooling suites: the measurement's name and the interpreter arguments.
SUITES = (
    ("dev-tooling", ("-B", "-m", "unittest", "discover", "-s", "scripts/dev", "-t", "scripts", "-p", "test_*.py")),
    ("gate-tooling", ("-B", "-m", "unittest", "scripts/test_gate.py", "scripts/test_claude_rm_hook.py")),
    ("native-qa-tooling", ("-B", "-m", "unittest", "discover", "-s", "scripts/native_qa", "-t", "scripts", "-p", "test_*.py")),
    ("ci-tests", ("-B", "-m", "unittest", "discover", "-s", "scripts/ci/tests", "-p", "test_*.py")),
)
HEARTBEAT = 30.0
# Seconds this driver gives the step, counted from its own start. The job's limit of 10 minutes
# is unchanged and still has the last word: this one is stricter, never wider, and nothing is
# run again after it.
DEADLINE = 510.0
# Seconds between SIGTERM and SIGKILL for a session that is being ended.
GRACE = 10.0
PREFIX = "[ci] "
# unittest's summary of a run, the whole line as it prints it, behind the collector's prefix. The
# count is bounded because the line is the tests' text: thousands of digits are no count.
RAN = re.compile(r"\[ci\] Ran (\d{1,9}) tests? in \d+\.\d{3}s")

# One process of the step: its measurement name and its command.
Job = collections.namedtuple("Job", "name command")


def say(text):
    print(f"tooling-suites: {text}", flush=True)


def plan(suites=SUITES):
    """The step's processes in the order their output is printed."""
    return [Job(name, (sys.executable, *arguments)) for name, arguments in suites]


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
    """End the processes that have no exit status yet; return those that were asked to end."""
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
        item.waiter.join(timeout=max(grace, 1.0))
    return asked


class Running:
    """One started process: its captured output, and its exit status and seconds once it has ended."""

    def __init__(self, name, argv, cwd, clock):
        self.name, self.exit, self.seconds, self.cut = name, None, None, False
        self.capture = tempfile.TemporaryFile()
        self.clock, self.started = clock, clock()
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
    """Start every (name, command) at once and wait until all of them have ended, or until `until`."""
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
                    say("the step's deadline has passed: ending " + ", ".join(item.name for item in late))
                for item in end_sessions(alive, grace, clock):
                    item.cut = True
                break
            alive[0].waiter.join(timeout=max(0.0, min(1.0, beat_at - now, *(() if until is None else (until - now,)))))
            if clock() >= beat_at:
                beat_at += beat
                say(f"+{clock() - origin:.0f} s: " + "; ".join(item.state() for item in running))
    except BaseException:
        end_sessions(running, grace, clock)
        raise
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    return running, passed


def quote(raw, metrics):
    """One captured line as it may be printed: behind the collector's prefix, on one line and harmless."""
    text = raw.decode("utf-8", errors="replace").rstrip("\n")
    if text.startswith(PREFIX):
        return PREFIX + metrics.harmless(metrics.ANSI.sub("", text[len(PREFIX):]))
    if len(raw) > metrics.MAX_LINE:
        return PREFIX + "[overlong diagnostic line omitted]"
    return PREFIX + metrics.harmless(metrics.sanitize(text))


def replay(item, metrics):
    """Print one process's output whole in a log group."""
    ran = problem = None
    print(f"::group::{item.name}")
    try:
        if item.waiter.is_alive():
            say(f"{item.name} could not be ended, so its output is not printed")
        else:
            item.capture.seek(0)
            for raw in item.capture:
                line = quote(raw, metrics)
                match = RAN.fullmatch(line)
                if match:
                    ran = max(ran or 0, int(match.group(1)))
                print(line)
    except Exception as error:
        problem = type(error).__name__
    finally:
        if not item.waiter.is_alive():
            item.capture.close()
        print("::endgroup::", flush=True)
    return ran, problem


def measured(job, directory):
    """One job's command under the collector, which keeps its sanitized log and timing record in `directory`."""
    return job.name, [sys.executable, str(METRICS), "measure", "--name", job.name, "--directory", str(directory),
                      "--", *job.command]


def run(directory, jobs, cwd=ROOT, beat=HEARTBEAT, deadline=DEADLINE, grace=GRACE, clock=time.monotonic):
    metrics = collector()
    directory.mkdir(parents=True, exist_ok=True)
    say(f"starting {len(jobs)} processes at once: {', '.join(job.name for job in jobs)}")
    begun = clock()
    until = begun + deadline
    running, passed = side_by_side([measured(job, directory) for job in jobs], cwd, beat, until, grace, clock)
    whole = clock() - begun
    replays = [replay(item, metrics) for item in running]
    counts = [count for count, _ in replays]
    for item, count in zip(running, counts):
        say(f"{item.name}: {'cut at the deadline, ' if item.cut else ''}exit {item.exit}, {item.elapsed():.1f} s, "
            f"{'no' if count is None else count} tests")
    say(f"{len(jobs)} processes side by side took {whole:.1f} s")
    failures = []
    for item in running:
        if item.cut:
            failures.append(f"{item.name} was cut at the deadline of {deadline:.0f} s, after {item.elapsed():.1f} s")
        elif item.exit != 0:
            failures.append(f"{item.name} exited {item.exit}")
    failures += [f"{item.name} ran no test" for item, count in zip(running, counts) if not count and not item.cut]
    failures += [f"the output of {item.name} could not be printed whole ({problem})"
                 for item, (_, problem) in zip(running, replays) if problem]
    failures += [f"the driver was sent {signal.Signals(signum).name} and passed it on" for signum in dict.fromkeys(passed)]
    for failure in failures:
        say(f"FAILED: {failure}")
    if failures:
        return 1
    say(f"all {len(jobs)} processes passed")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="mode", required=True)
    step = sub.add_parser("run", help="run the tooling suites side by side and fail when any of them does")
    step.add_argument("--directory", required=True, help="where `metrics.py measure` keeps each process's log and timing record")
    step.add_argument("--skip-ci-tests", action="store_true", help="omit ci-tests when a sibling Ubuntu job already runs scripts/ci/tests")
    args = parser.parse_args(argv)
    selected_suites = tuple(item for item in SUITES if not (args.skip_ci_tests and item[0] == "ci-tests"))
    return run(Path(args.directory).resolve(), plan(suites=selected_suites))


if __name__ == "__main__":
    sys.exit(main())
