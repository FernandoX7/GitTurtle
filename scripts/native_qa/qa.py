#!/usr/bin/env python3
"""GitTurtle native-QA tooling: isolated launches, frame comparison, privacy scan, build identity, display check.

  python3 scripts/native_qa/qa.py launch --binary B --fixture F --run-dir /abs/empty [--scenario S.json]
  python3 scripts/native_qa/qa.py compare BASE CANDIDATE [--mask status-timing] [--mask x0,y0,x1,y1]
  python3 scripts/native_qa/qa.py privacy scan FRAME... --templates /local/dir
  python3 scripts/native_qa/qa.py identity BASE [CANDIDATE]
  python3 scripts/native_qa/qa.py display-check [--display :1]

Exit status: 0 clean, 1 a finding (difference, privacy hit, identity problem,
foreign process), 2 a refusal, usage error or inconclusive check.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

if sys.version_info < (3, 11):
    raise SystemExit("qa.py requires Python 3.11 or newer")

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from native_qa import frames, identity, runenv, stores  # noqa: E402


def size(text: str) -> tuple[int, int]:
    width, _, height = text.partition("x")
    return int(width), int(height)


def key_value(text: str) -> tuple[str, str]:
    key, sep, value = text.partition("=")
    if not sep or not key:
        raise argparse.ArgumentTypeError(f"expected KEY=VALUE, got {text!r}")
    return key, value


def write_json(path: Path | None, payload) -> None:
    if path is not None:
        path.write_text(json.dumps(payload, indent=1))


# ---------- commands ----------
def launch(args) -> int:
    from native_qa import session

    steps = session.load_scenario(args.scenario)
    if args.preferences:
        preferences = stores.load_file(args.preferences)
    else:
        preferences = stores.store_text(args.theme, args.project)
    width, height = args.size
    run = session.Session(args.binary, args.fixture, args.run_dir, preferences, width=width, height=height,
                          display=args.display, scale=args.scale, for_commit=args.for_commit,
                          extra_env=dict(args.env), settle=args.settle)
    if args.scenario:
        run.log["header"]["scenario"] = dict(path=str(args.scenario),
                                             sha256=identity.sha256_file(args.scenario), steps=steps)
    monitor = None
    try:
        if args.monitor_portal:
            from native_qa import portal

            monitor = portal.FileChooserMonitor(run.dirs.root / "filechooser-dbus.log")
        run.launch()
        run.run(steps)
    except BaseException as error:
        run.log["error"] = repr(error)
        raise
    finally:
        code = run.close()
        if monitor is not None:
            monitor.stop()
    print(f"run directory: {run.dirs.root}")
    return 0 if code in (0, -15) and run.log["fixture_unchanged"] else 1


def compare(args) -> int:
    masks = [frames.parse_mask(spec) for spec in args.mask]
    reports = [frames.compare_files(base, candidate, masks)
               if base.exists() and candidate.exists()
               else dict(base=str(base), candidate=str(candidate), result="missing")
               for base, candidate in frames.pairs(args.base, args.candidate)]
    for report in reports:
        line = f"{Path(report['candidate']).name}: {report['result']}"
        if report.get("differing_pixels") is not None:
            line += f", {report['differing_pixels']} px outside masks, {report['masked_pixels']} px masked"
        print(line)
        for box in report.get("regions", []):
            print(f"    region {box}")
    write_json(args.json, reports)
    return 0 if all(report["result"] == "identical" for report in reports) else 1


def privacy_scan(args) -> int:
    from native_qa import privacy

    if bool(args.templates) == bool(args.text):
        raise SystemExit("give either --templates DIR or --text with --font")
    if args.templates:
        templates = privacy.load_templates(args.templates)
    else:
        if not args.font:
            raise SystemExit("--text needs --font (the face the app draws with, for example Ubuntu)")
        templates = privacy.render_templates(args.text, args.font, args.size or [13])
    report = privacy.scan(args.frames, templates, args.threshold, args.engine)
    for frame, result in report.items():
        best = max(result["templates"].items(), key=lambda item: item[1]["best"])
        print(f"{Path(frame).name}: {result['hits']} hit(s) >= {args.threshold}; best {best[0]} "
              f"{best[1]['best']:.2f} at {best[1]['at']} ({result['engine']})")
        for name, found in result["templates"].items():
            for x, y, score in found["hit_list"]:
                print(f"    HIT {name} at ({x},{y}) ncc {score:+.3f}")
    write_json(args.json, report)
    return 1 if any(result["hits"] for result in report.values()) else 0


def privacy_crop(args) -> int:
    from native_qa import privacy

    box = frames.parse_mask(args.box)
    if isinstance(box, str):
        raise ValueError("crop needs x0,y0,x1,y1")
    privacy.crop(args.frame, box, args.output)
    print(f"template written: {args.output}")
    return 0


def identity_command(args) -> int:
    entries = [identity.describe(binary) for binary in args.binaries]
    for entry in entries:
        info = entry["build_info"]
        print(f"{entry['path']}\n  sha256 {entry['sha256']}\n  source_revision {info['source_revision']} "
              f"source_tree {info['source_tree']} profile {info['profile']} target {info['target']}")
    found = identity.problems(entries, pair=len(entries) == 2, allow_dirty=args.allow_dirty)
    for problem in found:
        print(f"PROBLEM: {problem}")
    write_json(args.json, dict(binaries=entries, problems=found))
    return 1 if found else 0


def display_check(args) -> int:
    from native_qa import display

    status, lines = display.check(args.display, set(args.allow_pid))
    print("\n".join(lines))
    print({0: "display clear", 1: "FOREIGN processes or windows present", 2: "INCONCLUSIVE"}[status])
    return status


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = top.add_subparsers(dest="command", required=True)

    p = commands.add_parser("launch", help="one isolated launch: seed, start, drive a scenario, SIGTERM")
    p.add_argument("--binary", type=Path, required=True, help="absolute path of the executable under test")
    p.add_argument("--fixture", type=Path, required=True, help=f"repository to open, under {runenv.EVIDENCE_ROOT}/")
    p.add_argument("--run-dir", type=Path, required=True, help="absolute, absent or empty; holds HOME, XDG_* and captures")
    store = p.add_mutually_exclusive_group()
    store.add_argument("--theme", default="midnight", help="built-in theme key for a generated store (default midnight)")
    store.add_argument("--preferences", type=Path, help="a preferences.json to seed byte for byte")
    p.add_argument("--project", action="append", default=[], metavar="PATH[=NAME]",
                   help="add a project to the generated store (repeatable)")
    p.add_argument("--scenario", type=Path, help="JSON list of steps (default: settle, park, capture 00-launch)")
    p.add_argument("--size", type=size, default=(1000, 680), help="window size, default 1000x680")
    p.add_argument("--display", default=runenv.DEFAULT_DISPLAY)
    p.add_argument("--scale", default=runenv.DEFAULT_SCALE, help="GPUI_X11_SCALE_FACTOR (default 1)")
    p.add_argument("--settle", type=float, default=5.0, help="seconds after the resize before the first input")
    p.add_argument("--env", type=key_value, action="append", default=[], metavar="KEY=VALUE",
                   help="extra variable, for example GITTURTLE_GITHUB_FIXTURE=review")
    p.add_argument("--monitor-portal", action="store_true", help="record FileChooser D-Bus traffic in the run directory")
    p.add_argument("--for-commit", action="store_true",
                   help=f"refuse a fixture outside {runenv.EVIDENCE_ROOT}/ or a run directory under a home root")
    p.set_defaults(func=launch)

    p = commands.add_parser("compare", help="base versus candidate frames, with masks")
    p.add_argument("base", type=Path, help="a frame, or a directory of frames")
    p.add_argument("candidate", type=Path, help="a frame, or a directory of frames named like the base's")
    p.add_argument("--mask", action="append", default=[], metavar="NAME|x0,y0,x1,y1",
                   help=f"exclude a region from the verdict (named: {', '.join(frames.NAMED_MASKS)}); repeatable")
    p.add_argument("--json", type=Path, help="write the full report here")
    p.set_defaults(func=compare)

    p = commands.add_parser("privacy", help="template scan of frames for personal strings")
    actions = p.add_subparsers(dest="action", required=True)
    s = actions.add_parser("scan", help="exit 1 if any template matches a frame")
    s.add_argument("frames", type=Path, nargs="+", help="PNG/JPEG frames or directories of them")
    s.add_argument("--templates", type=Path, help="local, untracked directory of template PNGs")
    s.add_argument("--text", action="append", default=[], help="render this string as a template (repeatable)")
    s.add_argument("--font", type=Path, help="TrueType/OpenType file for --text")
    s.add_argument("--size", type=int, action="append", default=[], help="pixel size for --text (repeatable, default 13)")
    s.add_argument("--threshold", type=float, default=0.80, help="|ncc| that counts as a hit (default 0.80)")
    s.add_argument("--engine", choices=("auto", "c", "python"), default="auto")
    s.add_argument("--json", type=Path, help="write the full report here")
    s.set_defaults(func=privacy_scan)
    c = actions.add_parser("crop", help="cut a template from a frame that shows a personal string")
    c.add_argument("frame", type=Path)
    c.add_argument("box", help="x0,y0,x1,y1")
    c.add_argument("output", type=Path, help="template PNG, outside any tracked path")
    c.set_defaults(func=privacy_crop)

    p = commands.add_parser("identity", help="sha256 and --build-info; with two binaries, the pair refusals")
    p.add_argument("binaries", type=Path, nargs="+", metavar="BINARY", help="one binary, or BASE CANDIDATE")
    p.add_argument("--allow-dirty", action="store_true", help="do not flag a source_tree other than clean")
    p.add_argument("--json", type=Path, help="write the identities here")
    p.set_defaults(func=identity_command)

    p = commands.add_parser("display-check", help="date -u, running GitTurtle/driver processes and windows")
    p.add_argument("--display", default=runenv.DEFAULT_DISPLAY)
    p.add_argument("--allow-pid", type=int, action="append", default=[], help="a known process that is not foreign")
    p.set_defaults(func=display_check)
    return top


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    if args.command == "identity" and len(args.binaries) > 2:
        raise SystemExit("identity takes one binary or a BASE CANDIDATE pair")
    try:
        return args.func(args)
    except SystemExit as stop:
        if not isinstance(stop.code, str):
            raise
        print(stop.code, file=sys.stderr)  # refusals and preconditions, including runenv.Refusal
        return 2
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
