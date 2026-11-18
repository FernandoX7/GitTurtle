#!/usr/bin/env python3
"""Compare the tests two Rust CI job logs executed: libtest (`cargo test`) or nextest (stdlib only).

Accepts raw job logs (`gh api .../actions/jobs/<id>/logs`), `gh run view --log`
output for one job, or a diagnostics `tests.log`. Tests are keyed
`<kind>:<target> <name>`, where kind is lib, bin, test, bench, example or doc and
the target uses Cargo's crate spelling (`-` becomes `_`), so both runners'
reports compare directly.

Results are counted, not only named. The hosted wrapper (`metrics.py measure`)
redacts text shaped like a credential, so every test of a module such as
`secret_service` prints under one name: a key therefore has a number of executed
and a number of ignored results, and two sides are equal only when every key's
numbers are.

Each side of `compare` takes one job's log or several taken together: the shards
of one partitioned run (`cargo nextest run --partition slice:m/n`) on one
platform, one attempt each. A shard's log lists every test, the other shards'
as `SKIP`, so the shards' executed counts add and a key's ignored count is what
the shards list less what they executed.

Each log is first reconciled with nextest's own totals: it holds one run, from
its `Starting` line to its `Summary` line, and the result lines counted between
them must equal the Summary line's tests run, passed, failed and skipped. A
result line that is missing, repeated or printed by a test therefore makes the
log unreconciled instead of making two different runs compare equal. Lines
after the Summary line repeat the failures and are not counted.

`compare` exits 0 when the sides are identical, 1 when they differ and 3 when a
log does not reconcile, whatever the comparison then says. Its text report and
`list` escape every character outside printable text, as `--json` does: the
names come from logs that test output can shape.
"""

import argparse
from collections import Counter
import json
import re
import sys

ANSI = re.compile(r"(?:\x1b|\^\[)\[[0-?]*[ -/]*[@-~]")
PREFIX = re.compile(r"^(?:[^\t\n]*\t){0,2}\ufeff?(?:\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z ?)?(?:\[ci\] ?)?")
LIBTEST_BINARY = re.compile(r"^\s*Running (?P<unit>unittests )?(?P<source>\S+) \((?P<executable>[^)]*)\)\s*$")
LIBTEST_DOCS = re.compile(r"^\s*Doc-tests (?P<crate>\S+)\s*$")
LIBTEST_RESULT = re.compile(r"^test (?P<name>.+?) \.\.\. (?P<result>ok|ignored|FAILED)(?:,.*)?$")
# nextest 0.9.146, as three runs of `gitturtle-preview` and the hosted logs print it:
#     Starting 96 tests across 1 binary (97 tests skipped)
#         PASS [   0.010s] ( 1/96) gitturtle-preview animation::tests::spent_budget
#         SKIP [         ] (─────) gitturtle-preview animation::tests::caps_frames
#         SLOW [> 60.000s] (─────────) gitturtle::bin/gitturtle views::tests::slow
#      Summary [   3.263s] 96 tests run: 96 passed, 97 skipped
# A finished test's line carries its place in the run, `(n/N)`. A test outside
# the shard's slice prints the same SKIP line as one marked `#[ignore]`, and a
# progress line such as SLOW is ruled like a SKIP line: its test prints its
# result later.
NEXTEST_RESULT = re.compile(r"^\s*(?P<status>[A-Z][A-Z0-9 +/-]*?) \[[^\]]*\] (?:\[[^\]]*\] )?(?:\((?P<place>[^)]*)\) )?"
                            r"(?P<binary>[A-Za-z0-9_.:/-]+) (?P<name>\S+)\s*$")
NEXTEST_PLACE = re.compile(r"\s*\d+/\d+")
# The loose forms find every line that claims to open or close a run; the
# strict forms read the figures of the shapes observed.
NEXTEST_STARTING = re.compile(r"^\s*Starting \d+ tests? across ")
NEXTEST_STARTING_FIGURES = re.compile(r"^\s*Starting (?P<tests>\d+) tests? across \d+ (?:binary|binaries)"
                                      r"(?: \((?P<skipped>\d+) tests? skipped\))?\s*$")
NEXTEST_SUMMARY = re.compile(r"^\s*Summary \[")
NEXTEST_SUMMARY_FIGURES = re.compile(r"^\s*Summary \[[^\]]*\] (?P<run>\d+) tests? run: (?P<figures>\S.*?)\s*$")
# `3612 passed (2 slow), 1 failed, 27 skipped`
NEXTEST_FIGURE = re.compile(r"(?P<count>\d+) (?P<what>[a-z]+(?: [a-z]+)*)(?: \([^()]*\))?(?:, |$)")
KINDS = ("bin", "bench", "example", "test")


def crate(name):
    return name.replace("-", "_")


def libtest_key(unit, source, executable):
    stem = re.sub(r"(?:-[0-9a-f]{8,})?(?:\.exe)?$", "", re.split(r"[\\/]", executable.strip())[-1])
    if unit:
        return f"{'lib' if source.endswith('src/lib.rs') else 'bin'}:{crate(stem)}"
    top = source.split("/", 1)[0]
    kind = {"benches": "bench", "examples": "example"}.get(top, "test")
    return f"{kind}:{crate(stem)}"


def nextest_key(binary):
    package, _, target = binary.partition("::")
    if not target:
        return f"lib:{crate(package)}"
    kind, _, name = target.partition("/")
    return f"{kind}:{crate(name)}" if name and kind in KINDS else f"test:{crate(target)}"


def summary_figures(line):
    """The figures of a nextest Summary line, or None for a shape this tool has not been taught."""
    if not (match := NEXTEST_SUMMARY_FIGURES.match(line)):
        return None
    figures, position = {}, 0
    while position < len(match["figures"]):
        figure = NEXTEST_FIGURE.match(match["figures"], position)
        if not figure or figure["what"] in figures:
            return None
        figures[figure["what"]] = int(figure["count"])
        position = figure.end()
    return {"tests_run": int(match["run"]), **{what: figures.pop(what, 0) for what in ("passed", "failed", "skipped")},
            "other": figures}


def more_or_fewer(found, stated):
    return f"{abs(found - stated)} {'more' if found > stated else 'fewer'}"


def reconcile(statuses, skipped, starting, summary, runs, headers):
    """What keeps one log's counted result lines from being its runner's own totals."""
    if runs["misplaced"] or (runs["started"], runs["finished"]) not in ((0, 0), (1, 1)):
        return [f"{runs['started']} Starting and {runs['finished']} Summary lines of nextest"
                + (", a Summary line outside a run" if runs["misplaced"] else "")
                + ": a log holds one run, from its Starting line to its Summary line"]
    if not runs["started"]:
        return [] if headers else ["neither a nextest run nor a libtest header: the log holds no test run"]
    if starting is None or summary is None:
        return [f"a nextest line this tool cannot read: {runs['unread'][0].strip()}"]
    others = ", ".join(f"{status} {count}" for status, count in sorted(statuses.items()) if status not in ("PASS", "FAIL"))
    others = f"; other statuses: {others}" if others else ""
    problems = []
    for found, lines, figure, stated, note in (
        (sum(statuses.values()), "result lines of finished tests", "tests run", summary["tests_run"], ""),
        (statuses["PASS"], "PASS lines", "passed", summary["passed"], others),
        (statuses["FAIL"], "FAIL lines", "failed", summary["failed"], others),
        (skipped, "SKIP lines", "skipped", summary["skipped"], ""),
    ):
        if found != stated:
            problems.append(f"{lines}: {found}, {figure} in its Summary line: {stated} ({more_or_fewer(found, stated)}){note}")
    for figure, stated, name in (("tests", summary["tests_run"], "tests run"), ("skipped", summary["skipped"], "skipped")):
        if starting[figure] != stated:
            problems.append(f"{figure} in its Starting line: {starting[figure]}, {name} in its Summary line: {stated}")
    return problems


def extract(lines):
    """Count one log's results and reconcile them with the log's own totals.

    `ran` and `skipped` count nextest's result lines per key inside the log's
    run; `libtest_ran` and `libtest_ignored` are libtest's tests, each once,
    because a second line for a test is the output of a test that ran its own
    binary again. `nextest` holds the Summary line's figures, None without a
    readable run, and `problems` what does not reconcile.
    """
    ran, skipped, statuses = Counter(), Counter(), Counter()
    libtest_ran, libtest_ignored = set(), set()
    binary, headers, running, starting, summary = "unknown:", 0, False, None, None
    runs = {"started": 0, "finished": 0, "misplaced": False, "unread": []}
    for raw in lines:
        line = PREFIX.sub("", ANSI.sub("", raw.rstrip("\r\n")), count=1)
        if NEXTEST_STARTING.match(line):
            runs["started"] += 1
            running = True
            if match := NEXTEST_STARTING_FIGURES.match(line):
                starting = {"tests": int(match["tests"]), "skipped": int(match["skipped"] or 0)}
            else:
                runs["unread"].append(line)
        elif NEXTEST_SUMMARY.match(line):
            runs["finished"] += 1
            runs["misplaced"] |= not running
            running = False
            summary = summary_figures(line)
            if summary is None:
                runs["unread"].append(line)
        elif match := LIBTEST_BINARY.match(line):
            binary, headers = libtest_key(match["unit"], match["source"], match["executable"]), headers + 1
        elif match := LIBTEST_DOCS.match(line):
            binary, headers = f"doc:{crate(match['crate'])}", headers + 1
        elif match := LIBTEST_RESULT.match(line):
            name = match["name"]
            if not binary.startswith("doc:"):
                name = name.removesuffix(" - should panic")
            (libtest_ignored if match["result"] == "ignored" else libtest_ran).add(f"{binary} {name}")
        elif running and (match := NEXTEST_RESULT.match(line)) and not match["status"].startswith("SETUP"):
            key = f"{nextest_key(match['binary'])} {match['name']}"
            if match["place"] is not None and NEXTEST_PLACE.fullmatch(match["place"]):
                ran[key] += 1
                statuses[match["status"]] += 1
            elif match["status"] == "SKIP":
                skipped[key] += 1
    problems = reconcile(statuses, sum(skipped.values()), starting, summary, runs, headers)
    return {"ran": ran, "skipped": skipped, "libtest_ran": libtest_ran,
            # A test that also printed a result line in another state still executed.
            "libtest_ignored": libtest_ignored - libtest_ran,
            "nextest": summary if starting is not None and runs["started"] == runs["finished"] == 1 else None,
            "problems": problems}


def read(path):
    with open(path, encoding="utf-8", errors="replace") as stream:
        return extract(stream)


def executed_in(log, key):
    return log["ran"][key] + (key in log["libtest_ran"])


def unite(logs):
    """Take the logs of one side together: [(label, log)] in, the side's counts per key out.

    The executed counts add. A key's ignored count is what the logs with a
    nextest run list for it (each lists every test, as its result or as SKIP)
    less what they executed, plus libtest's ignored tests. `repeated` holds the
    keys executed more often than listed, which a partition never does, and
    `problems` says when the logs do not list the same tests.
    """
    executed, ignored, repeated, uneven = Counter(), Counter(), {}, {}
    for _, log in logs:
        executed.update(log["ran"])
        executed.update(dict.fromkeys(log["libtest_ran"], 1))
        ignored.update(dict.fromkeys(log["libtest_ignored"], 1))
    shards = [(label, log) for label, log in logs if log["ran"] or log["skipped"]]
    for key in sorted(set().union(*(log["ran"].keys() | log["skipped"].keys() for _, log in shards))):
        listed = {label: log["ran"][key] + log["skipped"][key] for label, log in shards}
        ran = sum(log["ran"][key] for _, log in shards)
        if len(set(listed.values())) > 1:
            uneven[key] = listed
        if ran > max(listed.values()):
            repeated[key] = {"listed": max(listed.values()), "executed": ran,
                             "logs": {label: log["ran"][key] for label, log in shards}}
        elif ran < max(listed.values()):
            ignored[key] += max(listed.values()) - ran
    problems = []
    if uneven:
        key, listed = next(iter(uneven.items()))
        problems.append("the logs do not list the same tests, so they are not the shards of one run on one platform: "
                        f"{len(uneven)} {'key differs' if len(uneven) == 1 else 'keys differ'}, first {key}{per_log(listed)}")
    runs = [log["nextest"]["tests_run"] for _, log in logs if log["nextest"]]
    return {"executed": executed, "ignored": ignored, "repeated": repeated, "problems": problems,
            "tests_run": sum(runs) if runs else None}


def compare_logs(before, after):
    """Compare a reference run's logs with a candidate run's, each side taken together."""
    logs = {"before": before, "after": after}
    sides = {side: unite(logs[side]) for side in logs}
    result = {"unreconciled": []}
    for side in logs:
        result[side] = {
            "executed": sum(sides[side]["executed"].values()), "ignored": sum(sides[side]["ignored"].values()),
            "tests_run": sides[side]["tests_run"],
            "logs": [{"log": label, "executed": sum(log["ran"].values()) + len(log["libtest_ran"]),
                      "skipped_or_ignored": sum(log["skipped"].values()) + len(log["libtest_ignored"]),
                      "nextest": log["nextest"]} for label, log in logs[side]],
        }
        result["unreconciled"] += [f"{label}: {problem}" for label, log in logs[side] for problem in log["problems"]]
        result["unreconciled"] += [f"{side}: {problem}" for problem in sides[side]["problems"]]
    for key in DIFFERENCES:
        result[key] = {}
    for state in ("executed", "ignored"):
        counts = {side: sides[side][state] for side in logs}
        for key in sorted(counts["before"].keys() | counts["after"].keys()):
            if counts["before"][key] == counts["after"][key]:
                continue
            entry = {side: counts[side][key] for side in logs}
            if state == "executed":
                entry |= {f"{side}_logs": {label: executed_in(log, key) for label, log in logs[side]} for side in logs}
            result[f"{state}_{'less' if entry['after'] < entry['before'] else 'more'}_after"][key] = entry
    for side in logs:
        result[f"executed_more_than_listed_{side}"] = sides[side]["repeated"]
    return result


# What each difference means, in the order it is reported.
DIFFERENCES = {
    "executed_less_after": "executed less often after than before (lost)",
    "executed_more_after": "executed more often after than before (run more than once, or new)",
    "ignored_less_after": "ignored less often after than before",
    "ignored_more_after": "ignored more often after than before",
    "executed_more_than_listed_before": "executed more often than the before logs list it (run more than once)",
    "executed_more_than_listed_after": "executed more often than the after logs list it (run more than once)",
}


def per_log(counts):
    return f" ({', '.join(f'{label} {count}' for label, count in counts.items())})" if len(counts) > 1 else ""


def printable(line):
    """One line for a terminal: every character outside printable text, and the backslash, as an escape.

    Test names and unread lines come from logs that a pull request's test
    output can shape. C0 and C1 controls (the ESC and the CSI or OSC that start
    a terminal sequence among them), DEL, line breaks and invisible formatting
    characters print as `\\x1b`, `\\x9b`, `\\u202e`, as `--json` escapes them.
    """
    return "".join(character if character.isprintable() and character != "\\"
                   else character.encode("unicode_escape").decode("ascii") for character in line)


def report(result):
    """The comparison as text and its exit status: counts, what does not reconcile, each difference, the verdict."""
    lines = []
    for side in ("before", "after"):
        logs = result[side]["logs"]
        together = f" ({len(logs)} logs taken together)" if len(logs) > 1 else ""
        lines.append(f"{side}: {result[side]['executed']} executed, {result[side]['ignored']} ignored{together}")
        if len(logs) > 1:
            lines += [f"  {log['log']}: {log['executed']} executed, {log['skipped_or_ignored']} skipped or ignored"
                      for log in logs]
    runs = {side: result[side]["tests_run"] for side in ("before", "after")}
    differences = [key for key in DIFFERENCES if result[key]]
    if None not in runs.values():
        totals = {side: per_log({log["log"]: log["nextest"]["tests_run"] for log in result[side]["logs"] if log["nextest"]})
                  for side in runs}
        differ = "" if runs["before"] == runs["after"] else f": {more_or_fewer(runs['after'], runs['before'])} after"
        lines.append(f"nextest tests run: {runs['before']} before{totals['before']}, "
                     f"{runs['after']} after{totals['after']}{differ}")
    different = bool(differences) or (None not in runs.values() and runs["before"] != runs["after"])
    if result["unreconciled"]:
        lines.append(f"not reconciled with the runner's own totals ({len(result['unreconciled'])}):")
        lines += [f"  {problem}" for problem in result["unreconciled"]]
    for key in differences:
        lines.append(f"{DIFFERENCES[key]} ({len(result[key])}):")
        for test, entry in result[key].items():
            if "listed" in entry:
                lines.append(f"  {test}: listed {entry['listed']}, executed {entry['executed']}{per_log(entry['logs'])}")
            else:
                lines.append(f"  {test}: {entry['before']} before{per_log(entry.get('before_logs', {}))}, "
                             f"{entry['after']} after{per_log(entry.get('after_logs', {}))}")
    lines.append("unreconciled" if result["unreconciled"] else "different" if different else "identical")
    # The only line breaks are the ones written here.
    return "\n".join(map(printable, lines)) + "\n", 3 if result["unreconciled"] else 1 if different else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    listing = commands.add_parser("list", help="print each executed and ignored result found in one side's logs")
    listing.add_argument("logs", nargs="+")
    difference = commands.add_parser(
        "compare", help="report the differences by count; exit 1 when not identical, 3 when a log does not reconcile")
    difference.add_argument("--before", nargs="+", required=True,
                            help="logs of the reference run, taken together (one per job or shard)")
    difference.add_argument("--after", nargs="+", required=True,
                            help="logs of the candidate run, taken together (one per job or shard)")
    difference.add_argument("--json", action="store_true", help="print the full result as JSON")
    args = parser.parse_args(argv)

    if args.command == "list":
        logs = [(path, read(path)) for path in args.logs]
        side = unite(logs)
        for state in ("executed", "ignored"):
            for key in sorted(side[state]):
                print("\n".join([printable(f"{state} {key}")] * side[state][key]))
        problems = [f"{label}: {problem}" for label, log in logs for problem in log["problems"]] + side["problems"]
        for problem in problems:
            print(printable(f"unreconciled: {problem}"), file=sys.stderr)
        return 3 if problems else 0
    result = compare_logs(*([(path, read(path)) for path in paths] for paths in (args.before, args.after)))
    text, status = report(result)
    print(json.dumps(result, indent=2) if args.json else text.rstrip("\n"))
    return status


if __name__ == "__main__":
    sys.exit(main())
