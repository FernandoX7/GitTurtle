#!/usr/bin/env python3
"""Compare the tests two Rust CI job logs executed: libtest (`cargo test`) or nextest (stdlib only).

Accepts raw job logs (`gh api .../actions/jobs/<id>/logs`), `gh run view --log`
output, or a diagnostics `tests.log`. Tests are keyed `<kind>:<target> <name>`,
where kind is lib, bin, test, bench, example or doc and the target uses Cargo's
crate spelling (`-` becomes `_`), so both runners' reports compare directly.

Each side of `compare` takes one log or several taken together, one per job: the
shards of a partitioned run (`cargo nextest run --partition slice:m/n`) are
united before the comparison. A shard's log lists the other shards' tests as
skipped, so a test counts as ignored only when no log of its side executed it,
and a test that more than one log of a side executed is reported as an error:
a partition runs each test once.
"""

import argparse
import json
import re
import sys

ANSI = re.compile(r"(?:\x1b|\^\[)\[[0-?]*[ -/]*[@-~]")
PREFIX = re.compile(r"^(?:[^\t\n]*\t){0,2}\ufeff?(?:\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z ?)?(?:\[ci\] ?)?")
LIBTEST_BINARY = re.compile(r"^\s*Running (?P<unit>unittests )?(?P<source>\S+) \((?P<executable>[^)]*)\)\s*$")
LIBTEST_DOCS = re.compile(r"^\s*Doc-tests (?P<crate>\S+)\s*$")
LIBTEST_RESULT = re.compile(r"^test (?P<name>.+?) \.\.\. (?P<result>ok|ignored|FAILED)(?:,.*)?$")
# `PASS [   0.012s] (  1/100) gitturtle-core path::to::test`; SKIP lines leave the
# duration blank and pad the counter with box-drawing characters.
NEXTEST_RESULT = re.compile(r"^\s*(?P<status>[A-Z][A-Z0-9 +/-]*?) \[[^\]]*\] (?:\[[^\]]*\] )?(?:\([^)]*\) )?"
                            r"(?P<binary>[A-Za-z0-9_.:/-]+) (?P<name>\S+)\s*$")
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


def extract(lines):
    """Return ({executed test keys}, {ignored or skipped test keys}) from one log."""
    executed, ignored = set(), set()
    binary = "unknown:"
    for raw in lines:
        line = PREFIX.sub("", ANSI.sub("", raw.rstrip("\r\n")), count=1)
        if match := LIBTEST_BINARY.match(line):
            binary = libtest_key(match["unit"], match["source"], match["executable"])
        elif match := LIBTEST_DOCS.match(line):
            binary = f"doc:{crate(match['crate'])}"
        elif match := LIBTEST_RESULT.match(line):
            name = match["name"]
            if not binary.startswith("doc:"):
                name = name.removesuffix(" - should panic")
            (ignored if match["result"] == "ignored" else executed).add(f"{binary} {name}")
        elif (match := NEXTEST_RESULT.match(line)) and not match["status"].startswith("SETUP"):
            key = f"{nextest_key(match['binary'])} {match['name']}"
            (ignored if match["status"] == "SKIP" else executed).add(key)
    # A test that also printed a result line in another state still executed.
    return executed, ignored - executed


def read(path):
    with open(path, encoding="utf-8", errors="replace") as stream:
        return extract(stream)


def unite(logs):
    """Take the logs of one run together: [(label, (executed, ignored))] in, one pair of sets out.

    Returns the united (executed, ignored) and, for each test that more than one
    log executed, the labels of those logs.
    """
    executed, ignored, ran_in = set(), set(), {}
    for label, (run, skipped) in logs:
        executed |= run
        ignored |= skipped
        for key in run:
            ran_in.setdefault(key, []).append(label)
    repeated = {key: ran_in[key] for key in sorted(ran_in) if len(ran_in[key]) > 1}
    return (executed, ignored - executed), repeated


def compare(before, after):
    (run_before, skip_before), (run_after, skip_after) = before, after
    return {
        "before": {"executed": len(run_before), "ignored": len(skip_before)},
        "after": {"executed": len(run_after), "ignored": len(skip_after)},
        "executed_only_before": sorted(run_before - run_after),
        "executed_only_after": sorted(run_after - run_before),
        "ignored_only_before": sorted(skip_before - skip_after),
        "ignored_only_after": sorted(skip_after - skip_before),
    }


def compare_logs(before, after):
    """Compare a reference run's logs with a candidate run's, each side taken together."""
    (united_before, repeated_before), (united_after, repeated_after) = unite(before), unite(after)
    result = compare(united_before, united_after)
    for side, logs, repeated in (("before", before, repeated_before), ("after", after, repeated_after)):
        result[side]["logs"] = [{"log": label, "executed": len(run), "skipped_or_ignored": len(skipped)}
                                for label, (run, skipped) in logs]
        result[f"executed_in_several_{side}"] = repeated
    return result


# What each difference means, in the order it is reported.
DIFFERENCES = {
    "executed_only_before": "executed before, not after",
    "executed_only_after": "executed after, not before",
    "ignored_only_before": "ignored before, not after",
    "ignored_only_after": "ignored after, not before",
    "executed_in_several_before": "executed in more than one before log",
    "executed_in_several_after": "executed in more than one after log",
}


def report(result):
    """The comparison as text and whether the sides differ: counts, each difference by name, the verdict."""
    lines = []
    for side in ("before", "after"):
        logs = result[side]["logs"]
        together = f" ({len(logs)} logs taken together)" if len(logs) > 1 else ""
        lines.append(f"{side}: {result[side]['executed']} executed, {result[side]['ignored']} ignored{together}")
        if len(logs) > 1:
            lines += [f"  {log['log']}: {log['executed']} executed, {log['skipped_or_ignored']} skipped or ignored"
                      for log in logs]
    differences = [key for key in DIFFERENCES if result[key]]
    for key in differences:
        lines.append(f"{DIFFERENCES[key]} ({len(result[key])}):")
        if isinstance(result[key], dict):
            lines += [f"  {test}: {', '.join(logs)}" for test, logs in result[key].items()]
        else:
            lines += [f"  {test}" for test in result[key]]
    lines.append("different" if differences else "identical")
    return "\n".join(lines) + "\n", bool(differences)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    listing = commands.add_parser("list", help="print the executed and ignored tests found in logs")
    listing.add_argument("logs", nargs="+")
    difference = commands.add_parser("compare", help="report the symmetric difference; exit 1 when not identical")
    difference.add_argument("--before", nargs="+", required=True,
                            help="logs of the reference run, taken together (one per job or shard)")
    difference.add_argument("--after", nargs="+", required=True,
                            help="logs of the candidate run, taken together (one per job or shard)")
    difference.add_argument("--json", action="store_true", help="print the full result as JSON")
    args = parser.parse_args(argv)

    if args.command == "list":
        (executed, ignored), _ = unite([(path, read(path)) for path in args.logs])
        for key in sorted(executed):
            print(f"executed {key}")
        for key in sorted(ignored):
            print(f"ignored {key}")
        return 0
    result = compare_logs(*([(path, read(path)) for path in paths] for paths in (args.before, args.after)))
    text, different = report(result)
    print(json.dumps(result, indent=2) if args.json else text.rstrip("\n"))
    return 1 if different else 0


if __name__ == "__main__":
    sys.exit(main())
