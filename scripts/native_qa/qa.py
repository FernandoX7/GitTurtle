#!/usr/bin/env python3
"""GitTurtle native-QA tooling: scenarios, isolated launches, frame comparison, privacy scan, build identity.

  python3 scripts/native_qa/qa.py scenario check SPEC
  python3 scripts/native_qa/qa.py scenario fixture SPEC [--fixtures-dir /abs/dir]
  python3 scripts/native_qa/qa.py scenario run SPEC --build base=EXE --build cand=EXE --out /abs/bundle [--fixture F]
  python3 scripts/native_qa/qa.py recheck SPEC --exe EXE --committed docs/evidence/TASK [--out /abs/dir]
  python3 scripts/native_qa/qa.py attestation --bundle B --task T --candidate SHA --base SHA [--recheck R] --out FILE
  python3 scripts/native_qa/qa.py launch --binary B --fixture F --run-dir /abs/empty [--scenario S.json] [--input mutter|xtest]
  python3 scripts/native_qa/qa.py compare BASE CANDIDATE [--mask status-timing] [--mask x0,y0,x1,y1]
  python3 scripts/native_qa/qa.py privacy scan FRAME... --templates /local/dir [--redacted] [--jobs N]
  python3 scripts/native_qa/qa.py privacy pack --templates /local/dir --output /local/templates.b64
  python3 scripts/native_qa/qa.py identity BASE [CANDIDATE]
  python3 scripts/native_qa/qa.py display-check [--display :1]

Exit status: 0 clean, 1 a finding (difference, privacy hit, identity problem,
foreign process), 2 a refusal, usage error or inconclusive check.
"""

from __future__ import annotations

import argparse
import json
import os
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


def build_role(text: str) -> tuple[str, Path]:
    role, sep, path = text.partition("=")
    if not sep or role not in ("base", "cand") or not path:
        raise argparse.ArgumentTypeError(f"expected base=EXE or cand=EXE, got {text!r}")
    return role, Path(path)


def full_sha(text: str) -> str:
    if not (40 <= len(text) <= 64 and all(c in "0123456789abcdef" for c in text)):
        raise argparse.ArgumentTypeError(f"expected a full lower-case commit SHA, got {text!r}")
    return text


# ---------- scenarios ----------
def scenario_check(args) -> int:
    from native_qa import scenario

    spec = scenario.load(args.spec)
    crops = scenario.committed(spec)
    print(f"{args.spec}: valid (version {spec['version']}, task {spec['task']}, sha256 {spec['sha256'][:12]})")
    print(f"  {len(spec['variants'])} variant(s), {len(spec['steps'])} steps, {len(spec['analyses'])} analyses, "
          f"{len(scenario.analysis_runs(spec))} evaluations, {len(crops)} committed crops:")
    for crop in crops:
        print(f"    {crop.name}  ({crop.crop or 'whole frame'})")
    return 0


def scenario_fixture(args) -> int:
    from native_qa import recipe, scenario

    spec = scenario.load(args.spec)
    if spec["fixture"] is None:
        raise SystemExit("refusing: the scenario has no fixture recipe")
    manifest = recipe.build(spec["fixture"], args.fixtures_dir)
    print(f"{'reused' if manifest['reused'] else 'built'} {manifest['root']}")
    print(f"  {recipe.summary(manifest)}")
    for rev, check in manifest["expect"].items():
        print(f"  expect {rev} {check['expected'][:12]}: found {check['found'][:12]}")
    return 0


def scenario_run(args) -> int:
    from native_qa import play

    builds = dict(args.build)
    if len(builds) != len(args.build):
        raise SystemExit("refusing: a role was given twice")
    return play.run(args.spec, builds, args.out, args.fixture, args.input, args.display, args.templates, args.jobs,
                    args.settle)


def recheck(args) -> int:
    from native_qa import play

    return play.recheck(args.spec, args.exe, args.committed, args.out, args.fixture, args.input, args.display,
                        args.settle)


def attestation(args) -> int:
    from native_qa import evidence

    if args.out.exists():
        raise SystemExit(f"refusing: {args.out} exists; an attestation is never overwritten")
    payload = evidence.attestation(args.bundle, args.task, args.candidate, args.base, args.recheck,
                                   args.evidence_commit, args.what, args.limitations)
    args.out.write_text(json.dumps(payload, indent=1) + "\n")
    print(f"attestation for {args.task} {args.candidate[:12]} written to {args.out}")
    for session in payload["sessions"]:
        print(f"  {session['utc']}: {session['what']}")
    return 0


# ---------- commands ----------
def launch(args) -> int:
    from native_qa import session

    from native_qa import mutter

    steps = session.load_scenario(args.scenario)
    backend = mutter.choose_input(args.input, mutter.process_argvs())
    if args.preferences:
        preferences = stores.load_file(args.preferences)
    else:
        preferences = stores.store_text(args.theme, args.project)
    width, height = args.size
    run = session.Session(args.binary, args.fixture, args.run_dir, preferences, width=width, height=height,
                          display=args.display, scale=args.scale, for_commit=args.for_commit,
                          extra_env=dict(args.env), settle=args.settle, backend=backend)
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
    return 0 if code in (0, -15) and run.log["fixture_unchanged"] and not run.restore_failures else 1


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

    if [bool(args.templates), bool(args.packed), bool(args.text)].count(True) != 1:
        raise SystemExit("give exactly one of --templates DIR, --packed FILE or --text with --font")
    if args.templates:
        templates = privacy.load_templates(args.templates, anonymous=args.redacted)
    elif args.packed:
        templates = privacy.unpack_templates(args.packed.read_text(encoding="ascii", errors="replace"))
    else:
        if not args.font:
            raise SystemExit("--text needs --font (the face the app draws with, for example Ubuntu)")
        templates = privacy.render_templates(args.text, args.font, args.size or [13])
    report = privacy.scan(args.frames, templates, args.threshold, args.engine, args.jobs)
    write_json(args.json, report)
    matched = sum(1 for result in report.values() if result["hits"])
    if args.redacted:
        # Automated callers: the frame and a verdict only, never a template's name, score or place.
        for frame, result in report.items():
            print(f"{frame}: {'MATCH' if result['hits'] else 'clean'}")
        print(f"privacy scan: {len(report)} image(s), {matched} matched a template")
        return 1 if matched else 0
    for frame, result in report.items():
        best = max(result["templates"].items(), key=lambda item: item[1]["best"])
        where = f" in frame {best[1]['frame']}" if result["frames"] > 1 else ""
        print(f"{Path(frame).name}: {result['hits']} hit(s) >= {args.threshold}; best {best[0]} "
              f"{best[1]['best']:.2f} at {best[1]['at']}{where} ({result['engine']})")
        for name, found in result["templates"].items():
            for x, y, score in found["hit_list"]:
                print(f"    HIT {name} at ({x},{y}) ncc {score:+.3f}")
    return 1 if matched else 0


def privacy_pack(args) -> int:
    from native_qa import privacy

    privacy.refuse_tracked(args.output, "packed template file")
    templates = privacy.load_templates(args.templates, anonymous=True)
    text = privacy.pack_templates(templates)
    summary = f"{len(templates)} template(s), {len(text)} bytes of base64 (secret limit {privacy.SECRET_LIMIT})"
    if len(text) > privacy.SECRET_LIMIT:
        print(f"not written: {summary}", file=sys.stderr)
        return 1
    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w", encoding="ascii") as output:
        output.write(text)
    print(f"packed {summary}")
    return 0


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
    return status


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = top.add_subparsers(dest="command", required=True)

    def live_options(p) -> None:
        p.add_argument("--fixture", type=Path, help="open this repository instead of building the spec's recipe")
        p.add_argument("--input", choices=("mutter", "xtest"), help="input backend, as for launch")
        p.add_argument("--display", default=runenv.DEFAULT_DISPLAY, help="X display (Mutter hosts: :0)")
        p.add_argument("--settle", type=float, default=5.0, help="seconds after each resize before the first step")

    p = commands.add_parser("scenario", help="declarative scenarios: validate, build the fixture, run a bundle")
    actions = p.add_subparsers(dest="action", required=True)
    s = actions.add_parser("check", help="validate a spec and list the crops it commits (no display)")
    s.add_argument("spec", type=Path)
    s.set_defaults(func=scenario_check)
    s = actions.add_parser("fixture", help="build or reuse the spec's fixture recipe (no display)")
    s.add_argument("spec", type=Path)
    s.add_argument("--fixtures-dir", type=Path, default=runenv.EVIDENCE_ROOT / "fixtures",
                   help=f"absolute parent of the fixture (default {runenv.EVIDENCE_ROOT}/fixtures)")
    s.set_defaults(func=scenario_fixture)
    s = actions.add_parser("run", help="every build x variant, then crops, manifest, analyses and privacy scan")
    s.add_argument("spec", type=Path)
    s.add_argument("--build", type=build_role, action="append", default=[], required=True, metavar="ROLE=EXE",
                   help="base=EXE and cand=EXE, release builds from their own target directories")
    s.add_argument("--out", type=Path, required=True, help=f"absolute, absent or empty bundle, e.g. "
                                                            f"{runenv.EVIDENCE_ROOT}/runs/<task>")
    s.add_argument("--templates", type=Path, help=".local/privacy/templates by default, as the gate finds it")
    s.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1), help="crops scanned at once")
    live_options(s)
    s.set_defaults(func=scenario_run)

    p = commands.add_parser("recheck", help="re-capture the candidate crops with EXE and compare their bytes")
    p.add_argument("spec", type=Path, help="the committed docs/evidence/<task>/scenario.json")
    p.add_argument("--exe", type=Path, required=True, help="the candidate executable to re-check")
    p.add_argument("--committed", type=Path, required=True, help="the directory holding the committed crops")
    p.add_argument("--out", type=Path, help=f"absolute, absent or empty (default {runenv.EVIDENCE_ROOT}/runs/"
                                            "<task>-recheck-<sha>-<utc>)")
    live_options(p)
    p.set_defaults(func=recheck)

    p = commands.add_parser("attestation", help="the native attestation JSON from a passing bundle")
    p.add_argument("--bundle", type=Path, required=True)
    p.add_argument("--task", required=True)
    p.add_argument("--candidate", type=full_sha, required=True, help="the attested candidate's full SHA")
    p.add_argument("--base", type=full_sha, required=True, help="the candidate's base commit")
    p.add_argument("--recheck", type=Path, help="recheck.json (or its directory) for a rebuilt candidate")
    p.add_argument("--evidence-commit", type=full_sha, help="the commit that holds the frames")
    p.add_argument("--what", help="replace the spec's summary in the session line")
    p.add_argument("--limitations", help="replace the spec's limitations")
    p.add_argument("--out", type=Path, required=True, help="a new file")
    p.set_defaults(func=attestation)

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
    p.add_argument("--input", choices=("mutter", "xtest"),
                   help="input backend; default mutter where XWayland runs with -enable-ei-portal (XTest is "
                        "refused there), otherwise xtest")
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
    s.add_argument("--packed", type=Path, help="a template set written by `privacy pack`")
    s.add_argument("--text", action="append", default=[], help="render this string as a template (repeatable)")
    s.add_argument("--font", type=Path, help="TrueType/OpenType file for --text")
    s.add_argument("--size", type=int, action="append", default=[], help="pixel size for --text (repeatable, default 13)")
    s.add_argument("--threshold", type=float, default=0.80, help="|ncc| that counts as a hit (default 0.80)")
    s.add_argument("--engine", choices=("auto", "c", "python"), default="auto")
    s.add_argument("--jobs", type=int, default=1, help="frames scanned at once (default 1)")
    s.add_argument("--redacted", action="store_true",
                   help="print each frame's verdict only, never a template's name, score or position")
    s.add_argument("--json", type=Path, help="write the full report here")
    s.set_defaults(func=privacy_scan)
    k = actions.add_parser("pack", help="pack a template directory into one base64 file for a CI secret")
    k.add_argument("--templates", type=Path, required=True, help="local, untracked directory of template PNGs")
    k.add_argument("--output", type=Path, required=True, help="the packed file, outside any tracked path")
    k.set_defaults(func=privacy_pack)
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

    p = commands.add_parser("display-check", help="date -u, GitTurtle/driver processes and windows, session lock")
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
