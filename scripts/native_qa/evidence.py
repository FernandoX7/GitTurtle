"""The evidence a scenario leaves: committed crops and their manifest, analyses, privacy scan, re-check, attestation.

A bundle (`qa.py scenario run --out BUNDLE`) holds:

  scenario.json          the spec, byte for byte
  fixture-manifest.json  the recipe build's manifest (absent with --fixture)
  <role>/<variant>/      one launch's run directory: flow-log.json, app.log, captures/, marks/ and
                         probes/<probe>/ (probe.json and every frame it kept; never committed)
  commit/                the crops to commit, under their committed names
  commit-manifest.json   per crop: name, sha256, bytes, size, role, variant, capture, box, layout (a scaled or
                         composed crop's boxes, scale, gap and fill colour), the window it is cut from, what
                         it shows
  analysis.json          every analysis with its numbers, verdict and the scenario's expectation (true,
                         false, or "record": measured and written, never a finding)
  run.json               builds, host, fixture, launches, privacy scan and the verdict

A re-check directory (`qa.py recheck`) holds scenario.json, cand/<variant>/,
commit/ and recheck.json. Everything here reads files only; `play.py` drives
the app.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path

from . import analysis, frames, identity, scenario

COMMIT = "commit"


def write_json(path: Path, payload) -> None:
    partial = path.with_name(f".{path.name}.partial")
    partial.write_text(json.dumps(payload, indent=1, default=str) + "\n")
    os.replace(partial, path)


def raw_frame(root: Path, role: str, variant: scenario.Variant, capture: str) -> Path:
    return root / role / variant.id / "captures" / f"{capture}.png"


def probe_dir(root: Path, role: str, variant: scenario.Variant, probe: str) -> Path:
    return root / role / variant.id / "probes" / probe


def load_probe(directory: Path, root: Path) -> dict | None:
    """A probe's record (`session.Session.probe`) with an `image` loader on every frame and each frame's `file`
    relative to `root`, as `analysis.probe_ring` and `probe_endpoints` read it; None when it was never written."""
    from PIL import Image

    path = directory / "probe.json"
    if not path.is_file():
        return None
    record = json.loads(path.read_text())
    for press in record["presses"]:
        for frame in press["frames"]:
            source = directory / frame["file"]

            def load(source=source):
                with Image.open(source) as image:
                    return image.convert("RGB")

            frame.update(image=load, file=str(source.relative_to(root)))
    return record


def cut(source: Path, box, output: Path, layout: dict | None = None) -> dict:
    """Crop `source` (the whole frame when `box` is None) into `output` as an RGB PNG; its digest and size.

    A scaled or composed crop (`layout`, `scenario.crop_layout`) is composed
    by `frames.compose` instead, and its boxes, scale, gap and the fill colour
    it used are returned as `layout`. The bytes depend on the pixels and on
    the Pillow and zlib versions only, so a re-capture with the same tooling
    reproduces a committed crop exactly.
    """
    from PIL import Image

    described = None
    with Image.open(source) as image:
        frame = image.convert("RGB")
        if layout is not None:
            cropped, fill = frames.compose(frame, layout["boxes"], layout["scale"], layout["gap"], layout["fill"])
            described = dict(boxes=[list(item) for item in layout["boxes"]], scale=layout["scale"], gap=layout["gap"],
                             fill=None if fill is None else list(fill))
        else:
            cropped = frame if box is None else frame.crop(tuple(box))
        cropped.save(output, optimize=True)
        size = list(cropped.size)
    data = output.read_bytes()
    return dict(sha256=hashlib.sha256(data).hexdigest(), bytes=len(data), size=size, layout=described)


def write_crops(root: Path, crops: list[scenario.Crop], destination: Path) -> list[dict]:
    """Cut every planned crop from its launch's raw capture into `destination`; manifest entries in plan order.

    Every raw frame must have the size of the window the scenario has at that
    capture (`Crop.window`, which its name carries): Pillow pads a crop that
    reaches past a smaller frame with black instead of failing.
    """
    from PIL import Image

    destination.mkdir()
    entries = []
    for crop in crops:
        source = raw_frame(root, crop.role, crop.variant, crop.capture)
        if not source.is_file():
            raise SystemExit(f"capture {crop.capture} of {crop.role} {crop.variant.id} is missing ({source})")
        with Image.open(source) as image:
            if tuple(image.size) != tuple(crop.window):
                raise SystemExit(f"capture {crop.capture} of {crop.role} {crop.variant.id} is {image.size[0]}x"
                                 f"{image.size[1]}, not the {crop.window[0]}x{crop.window[1]} window the scenario has "
                                 "at that capture; nothing cropped")
        info = cut(source, crop.box, destination / crop.name, crop.layout)
        entries.append(dict(name=crop.name, sha256=info["sha256"], bytes=info["bytes"], size=info["size"],
                            role=crop.role, variant=crop.variant.describe(), capture=crop.capture, crop=crop.crop,
                            box=list(crop.box) if crop.box else None, layout=info["layout"],
                            window=list(crop.window), shows=crop.shows,
                            source=str(source.relative_to(root)), source_sha256=identity.sha256_file(source)))
    return entries


def commit_manifest(spec: dict, entries: list[dict]) -> dict:
    """The manifest: every window size the scenario opens at or resizes to, in order of first use (`[[1000, 680]]`
    for one that never resizes), and the frames, each with the window it was cut from."""
    return dict(version=1, task=spec["task"], scenario_sha256=spec["sha256"],
                windows=[list(size) for size in scenario.windows(spec)], frames=entries)


def run_analyses(spec: dict, root: Path) -> dict:
    """Every analysis of the spec over the bundle's raw captures and probes, with the expectation it is held to.

    An analysis expected to `record` is measured and written like the others
    but always counts as expected, so a base's cut frames are on record
    without failing the run; `recorded` counts them.
    """
    from PIL import Image

    cache: dict = {}

    def frame(path: Path):
        if path not in cache:
            if path.is_file():
                with Image.open(path) as image:
                    cache[path] = image.convert("RGB")
            else:
                cache[path] = None
        return cache[path]

    logs: dict = {}

    def readings(role: str, variant: scenario.Variant) -> dict:
        """A launch's readings by label, from its flow-log.json (none when it is missing or unreadable)."""
        if (role, variant.id) not in logs:
            try:
                found = json.loads((root / role / variant.id / "flow-log.json").read_text()).get("readings", [])
            except (OSError, ValueError, AttributeError):
                found = []
            logs[role, variant.id] = {item.get("label"): item for item in found if isinstance(item, dict)}
        return logs[role, variant.id]

    sizes: dict = {}

    def mismatched(entry: dict, resolve, role, variant) -> list[str]:
        """Each frame the analysis reads whose size is not the window the scenario has at its capture, so a box
        validated against that window could fall outside it (a probe's region is checked when it runs)."""
        if entry["kind"] in analysis.PROBE_KINDS:
            return []
        if variant.id not in sizes:
            sizes[variant.id] = scenario.capture_windows(spec, variant)
        found = []
        for ref in analysis.frame_refs(entry):
            image, want = resolve(ref), sizes[variant.id].get(scenario.split_ref(ref, role)[1])
            if image is not None and want is not None and tuple(image.size) != tuple(want):
                found.append(f"frame {ref} is {image.size[0]}x{image.size[1]}, not the {want[0]}x{want[1]} window "
                             "the scenario has at that capture")
        return found

    results = []
    for entry, role, variant in scenario.analysis_runs(spec):
        def resolve(ref, role=role, variant=variant, probe=entry["kind"] in analysis.PROBE_KINDS):
            named, name = scenario.split_ref(ref, role)
            if probe:
                return load_probe(probe_dir(root, named, variant, name), root)
            return frame(raw_frame(root, named, variant, name))

        def reading(ref, role=role, variant=variant):
            named, label = scenario.split_ref(ref, role)
            return readings(named, variant).get(label)

        wrong = mismatched(entry, resolve, role, variant)
        result = dict(passed=False, inconclusive=True, reasons=wrong) if wrong else \
            analysis.evaluate(entry, resolve, reading)
        want = scenario.expected(entry, role)
        inconclusive = bool(result.get("inconclusive"))  # a truncated or unsettled probe meets neither verdict
        results.append(dict(name=entry["name"], kind=entry["kind"], role=role, variant=variant.id,
                            frames=analysis.frame_refs(entry), expected=want, passed=result["passed"],
                            inconclusive=inconclusive,
                            as_expected=want == scenario.RECORD or (not inconclusive and result["passed"] == want),
                            note=entry.get("note", ""), result=result))
    unexpected = [f"{r['name']} ({r['role'] or 'both roles'}, {r['variant']}): "
                  f"{'inconclusive' if r['inconclusive'] else 'passed' if r['passed'] else 'failed'}, "
                  f"expected {'pass' if r['expected'] else 'fail'}"
                  for r in results if not r["as_expected"]]
    return dict(version=1, task=spec["task"], scenario_sha256=spec["sha256"], total=len(results),
                as_expected=len(results) - len(unexpected),
                recorded=sum(1 for r in results if r["expected"] == scenario.RECORD), unexpected=unexpected,
                results=results)


def privacy_scan(paths: list[Path], loaded: dict, jobs: int = 1) -> dict:
    """The redacted scan of exactly these files with `loaded` templates (`privacy.load_templates(..., anonymous=True)`):
    each one's verdict and the sha256 it had when scanned."""
    from . import privacy

    started = time.perf_counter()
    report = privacy.scan(list(paths), loaded, jobs=jobs)
    verdicts = {Path(path).name: ("MATCH" if result["hits"] else "clean") for path, result in report.items()}
    return dict(command="qa.py privacy scan --redacted (the default or given templates) on the crops in commit/",
                templates=len(loaded), frames=len(verdicts), matched=sum(v == "MATCH" for v in verdicts.values()),
                seconds=round(time.perf_counter() - started, 2), verdicts=verdicts,
                scanned_sha256={Path(path).name: identity.sha256_file(Path(path)) for path in paths})


# ---------- re-check ----------
def compare_committed(crops: list[scenario.Crop], recaptured: Path, committed: Path) -> dict:
    """Each re-captured crop against the committed file of the same name.

    `identical` means the same bytes. `pixels-identical` (same pixels, other
    bytes: another encoder) and `different` list the pixel difference; a
    committed candidate crop the spec does not produce is `not-in-spec`, so a
    re-check never passes by skipping a frame.
    """
    results = []
    for crop in crops:
        mine, theirs = recaptured / crop.name, committed / crop.name
        entry = dict(name=crop.name, variant=crop.variant.id, capture=crop.capture)
        if not theirs.is_file():
            results.append(dict(entry, result="missing", detail="no committed file of this name"))
            continue
        if not mine.is_file():
            results.append(dict(entry, result="not-recaptured"))
            continue
        a, b = theirs.read_bytes(), mine.read_bytes()
        entry.update(committed_sha256=hashlib.sha256(a).hexdigest(), recaptured_sha256=hashlib.sha256(b).hexdigest())
        if a == b:
            results.append(dict(entry, result="identical"))
            continue
        report = frames.compare_files(theirs, mine)
        if report["result"] == "size-mismatch":
            results.append(dict(entry, result="size-mismatch", committed_size=report["base_size"],
                                recaptured_size=report["candidate_size"]))
        elif report["result"] == "identical":
            results.append(dict(entry, result="pixels-identical", differing_pixels=0))
        else:
            results.append(dict(entry, result="different", differing_pixels=report["differing_pixels"],
                                bbox=report["bbox"], regions=report["regions"][:analysis.MAX_REGIONS]))
    produced = {crop.name for crop in crops}
    prefix = scenario.COMMITTED_PREFIX["cand"] + "-"
    for path in sorted(committed.glob(f"{prefix}*.png")):
        if path.name not in produced:
            results.append(dict(name=path.name, result="not-in-spec",
                                detail="a committed candidate crop that this scenario does not produce"))
    verdict = "identical" if results and all(r["result"] == "identical" for r in results) else "different"
    return dict(verdict=verdict, results=results)


# ---------- attestation ----------
def utc_span(starts: list[str], ends: list[str]) -> str:
    """`2026-10-01 22:33-22:40` from ISO UTC stamps; the end keeps its date when the session crosses midnight."""
    first, last = min(starts), max(ends)
    day, clock = first[:10], first[11:16]
    return f"{day} {clock}-{last[11:16]}" if last[:10] == day else f"{day} {clock}-{last[:10]} {last[11:16]}"


def build_summary(entry: dict) -> dict:
    info = entry["build_info"]
    return dict(path=entry["path"], sha256=entry["sha256"], source_revision=info.get("source_revision"),
                source_tree=info.get("source_tree"), profile=info.get("profile"), target=info.get("target"))


def load_recheck(path: Path) -> tuple[Path, dict]:
    path = Path(path)
    if path.is_dir():
        path = path / "recheck.json"
    return path, json.loads(path.read_text())


def fixture_line(run: dict) -> str:
    """The attestation's fixture sentence: the build, whether every launch left it unchanged and, for a scenario that
    writes, whether each launch's own copy changed as declared."""
    fixture, launches = run["fixture"], run["launches"]
    unchanged = all(l.get("fixture_unchanged") for l in launches)
    line = f"{fixture['summary']}, {'unchanged by every launch' if unchanged else 'CHANGED by a launch'}"
    if fixture.get("writes"):
        declared = all(l.get("writes", {}).get("verdict", {}).get("result") == "pass" for l in launches)
        line += (f"; each launch wrote only to its own fresh copy at {fixture['writes']['copy']}, which "
                 f"{'changed exactly as the scenario declares' if declared else 'did NOT change as declared'}")
    return line


def minimum_line(launches: list[dict]) -> str | None:
    """How the launches lowered the window's WM_NORMAL_HINTS minimum (`x11.Driver.lower_minimum`), from what each
    read before and after; None when none did. A refused record (`refused`, kept for diagnosis) lowered nothing."""
    changes: dict[str, int] = {}
    for launch in launches:
        record = launch.get("window_minimum")
        if record and record.get("applied") and not record.get("refused"):
            was, now = record["original"]["min_size"], record["applied"]["min_size"]
            change = f"from {was[0]}x{was[1]} to {now[0]}x{now[1]}"
            changes[change] = changes.get(change, 0) + 1
    if not changes:
        return None
    every = len(changes) == 1 and sum(changes.values()) == len(launches)
    told = (next(iter(changes)) + " in every launch" if every
            else "; ".join(f"{change} in {count} of {len(launches)} launches" for change, count in changes.items()))
    return f"window WM_NORMAL_HINTS minimum lowered {told}, read back before the first resize"


def attestation(bundle: Path, task: str, candidate: str, base: str, recheck: Path | None = None,
                evidence_commit: str | None = None, what: str | None = None,
                limitations: str | None = None) -> dict:
    """The native attestation for `candidate`, derived from a passing bundle and, for a rebuilt candidate, its re-check.

    Identity, times, host, input, fixture, frames and verdicts come from the
    files; prose comes from the spec (`summary`, `limitations`) unless
    `what` or `limitations` replace it. Refuses a bundle or re-check that did
    not pass, another task's bundle, files written for another scenario, a
    privacy scan that did not cover exactly the manifest's crops, a re-check
    of other crops than the bundle's candidate crops, and a candidate other
    than the attested executable's source revision.
    """
    bundle = Path(bundle)
    spec = scenario.load(bundle / "scenario.json")
    if spec["task"] != task:
        raise SystemExit(f"refusing: the bundle's scenario is for {spec['task']}, not {task}")
    run = json.loads((bundle / "run.json").read_text())
    if run.get("verdict", {}).get("result") != "pass":
        raise SystemExit(f"refusing: the bundle did not pass ({run.get('verdict')})")
    manifest = json.loads((bundle / "commit-manifest.json").read_text())
    analyses = json.loads((bundle / "analysis.json").read_text())
    for name, payload in (("run.json", run), ("commit-manifest.json", manifest), ("analysis.json", analyses)):
        if payload.get("scenario_sha256") != spec["sha256"]:
            raise SystemExit(f"refusing: {name} records scenario {payload.get('scenario_sha256')}, not the bundle's "
                             f"scenario.json {spec['sha256']}")
    crops = {frame["name"]: frame["sha256"] for frame in manifest["frames"]}
    scanned = run.get("privacy", {})
    if scanned.get("scanned_sha256") != crops or set(scanned.get("verdicts", {}).values()) - {"clean"}:
        raise SystemExit("refusing: the privacy scan in run.json did not cover exactly the manifest's crops, clean")
    builds = run["builds"]
    if "cand" not in builds:
        raise SystemExit("refusing: the bundle has no candidate build")
    launches = run["launches"]
    sessions = []
    revisions = {role: (entry["build_info"].get("source_revision") or "")[:7] for role, entry in builds.items()}
    shown = what or spec["summary"] or "the scenario's captures"
    pair = f"base {revisions['base']} vs candidate {revisions['cand']}" if "base" in builds else \
        f"candidate {revisions['cand']}"
    recorded = analyses.get("recorded", 0)
    sessions.append(dict(utc=utc_span([l["started_utc"] for l in launches], [l["ended_utc"] for l in launches]),
                         what=f"{pair}: {shown}; {len(manifest['frames'])} crops privacy-scanned clean "
                              f"({run['privacy']['frames']} scanned, {run['privacy']['matched']} matched); "
                              f"{analyses['as_expected']} of {analyses['total']} analyses as the scenario expects"
                              + (f" ({recorded} recorded without a verdict)" if recorded else "")))
    attested = build_summary(builds["cand"])
    comparison = {}
    if "base" in builds:
        comparison["base"] = dict(commit=builds["base"]["build_info"].get("source_revision"),
                                  sha256=builds["base"]["sha256"])
    where = str(bundle)
    if recheck is not None:
        path, checked = load_recheck(recheck)
        if checked.get("scenario_sha256") != spec["sha256"]:
            raise SystemExit("refusing: the re-check ran another scenario spec than the bundle")
        if checked.get("verdict") != "identical":
            raise SystemExit(f"refusing: the re-check found differences ({checked.get('verdict')})")
        # The re-check must have compared against exactly this bundle's candidate crops, byte for byte.
        bundle_cand = {f["name"]: f["sha256"] for f in manifest["frames"] if f["role"] == "cand"}
        rechecked = {r.get("name"): r.get("committed_sha256") for r in checked.get("results", [])
                     if r.get("result") == "identical"}
        if rechecked != bundle_cand or len(checked.get("results", [])) != len(bundle_cand):
            raise SystemExit("refusing: the re-check did not compare exactly this bundle's candidate crops "
                             "(same names and sha256) with identical results")
        comparison["first_candidate"] = dict(commit=attested["source_revision"], sha256=attested["sha256"],
                                             note="the bundle's candidate; the attested executable was re-checked "
                                                  "against its committed crops")
        attested = build_summary(checked["executable"])
        count = sum(1 for r in checked["results"] if r["result"] == "identical")
        lowered = minimum_line(checked["launches"])
        sessions.append(dict(utc=utc_span([l["started_utc"] for l in checked["launches"]],
                                          [l["ended_utc"] for l in checked["launches"]]),
                             what=f"re-check of {(attested['source_revision'] or '')[:7]}: the {count} candidate "
                                  f"crops re-captured byte-identical (sha256) to the committed "
                                  f"{checked['committed']}/candidate-*.png" + (f"; {lowered}" if lowered else "")))
        where += f" (re-check in {path.parent})"
    if attested["source_revision"] != candidate:
        raise SystemExit(f"refusing: the attested executable was built from {attested['source_revision']}, "
                         f"not the candidate {candidate}")
    sizes = sorted({f["variant"]["text_size"] for f in manifest["frames"] if f["variant"]["text_size"]})
    windows = sorted({tuple(f["window"]) for f in manifest["frames"] if f.get("window")})
    lowered = minimum_line(launches)
    return dict(
        task=task, kind="native", candidate=candidate, base=base, attested_executable=attested,
        comparison_builds=comparison,
        host=run["host"]["description"] + (f"; {lowered}" if lowered else ""),
        input=("qa.py scenario run through Mutter RemoteDesktop with X focus verified; never XTest"
               if run["host"]["input"] == "mutter" else "qa.py scenario run through XTest"),
        fixtures=fixture_line(run),
        sessions=sorted(sessions, key=lambda session: session["utc"]), evidence_commit=evidence_commit,
        committed_frames=f"docs/evidence/{task}/ ({len(manifest['frames'])} crops"
                         + (f", {' and '.join(map(str, sizes))} pt" if sizes else "")
                         + (f", windows {', '.join(f'{w}x{h}' for w, h in windows)}" if len(windows) > 1 else "")
                         + ") and its scenario.json",
        frames=[dict(name=f["name"], sha256=f["sha256"]) for f in manifest["frames"]],
        scenario=dict(committed_path=f"docs/evidence/{task}/scenario.json", sha256=spec["sha256"]),
        analyses=dict(total=analyses["total"], as_expected=analyses["as_expected"], recorded=recorded),
        bundle=where, limitations=limitations or spec["limitations"] or "none recorded")
