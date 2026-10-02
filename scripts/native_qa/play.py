"""Drive a scenario through the real app: one isolated launch per build role and variant, then the bundle.

Every launch goes through `session.Session` exactly as `qa.py launch
--for-commit` does: its own empty run directory and seeded store, the window
found by PID, Mutter input with X focus verified, nothing sent while the
desktop is locked, SIGTERM to the launched PID only, and the fixture's state
compared before and after. Steps run identically for every role; a capture
limited to some roles is still grabbed in all of them and only committed for
those. A failed guard stops the launch (after its `on_fail` keys) and the run;
nothing is retried.

Exit status, as for the other commands: 0 every launch passed, the analyses
met their expectations and the privacy scan is clean (or, for a re-check,
every crop is byte-identical); 1 a finding; 2 a refusal or an inconclusive run.
"""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
import time
import zlib
from pathlib import Path

from . import analysis, evidence, identity, recipe, runenv, scenario, stores


class Inconclusive(SystemExit):
    """A guard failed: the app is not where the scenario expects, so nothing further is sent."""

    def __init__(self, message: str) -> None:
        super().__init__(f"inconclusive: {message}")


def utc() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def session_step(step: dict, role: str) -> dict:
    """The step as `Session.run` reads it; a capture's role-specific `shows` becomes its logged `what`."""
    if "capture" in step:
        what = scenario.shows_for(step, role) if role in step["roles"] and step["commit"] else \
            f"not committed for this role ({', '.join(step['roles']) if step['commit'] else 'aux'})"
        return dict(capture=step["capture"], what=what, keep_pointer=step["keep_pointer"],
                    settle=step["settle"], stable_within=step["stable_within"], quiet=step["quiet"])
    return {key: value for key, value in step.items() if key not in ("when", "index")}


def require_settled(run, step: dict, role: str) -> None:
    """A frame to commit must come from a settled window: one that never settled may not reproduce."""
    if not (step["commit"] and role in step["roles"] and step["stable_within"]):
        return
    if run.log["captures"][-1].get("stable_s") is None:
        raise Inconclusive(f"capture {step['capture']} (step {step['index']}) is committed, but the window did not "
                           f"settle within {step['stable_within']} s")


def guard(run, step: dict) -> None:
    def resolve(ref):
        return run.driver.snap() if ref == scenario.NOW else run.frames.get(ref)

    result = analysis.evaluate(step["guard"], resolve)
    note = step.get("note", "")
    run.log["scenario"]["guards"].append(dict(step=step["index"], note=note, passed=result["passed"], result=result))
    print(f"guard (step {step['index']}) {'ok' if result['passed'] else 'FAILED'}: {note}", flush=True)
    if not result["passed"]:
        for key in step["on_fail"]:
            run.run([key])
        raise Inconclusive(f"guard at step {step['index']} ({note}) failed: {'; '.join(result['reasons'])}")


def launch(spec: dict, role: str, variant: scenario.Variant, binary: Path, fixture: Path, run_dir: Path,
           options: dict) -> dict:
    """One launch of `binary` for `variant`; its record, with `error` set when it did not finish cleanly."""
    from . import session

    store = stores.store_text(variant.palette, settings=scenario.store_settings(spec, variant))
    # The digest flow-log.json's header records too: the merged store this launch starts from.
    record = dict(role=role, variant=variant.id, run_dir=str(run_dir), store_sha256=identity.sha256_bytes(store),
                  error=None, refusal=False)
    width, height = spec["window"]
    try:
        run = session.Session(binary, fixture, run_dir, store, width=width, height=height,
                              display=options["display"], scale="1", for_commit=True, extra_env=spec["env"],
                              settle=options["settle"], backend=options["backend"])
    except SystemExit as error:
        return dict(record, error=str(error), refusal=True, started_utc=utc(), ended_utc=utc())
    run.log["scenario"] = dict(task=spec["task"], sha256=spec["sha256"], role=role, variant=variant.describe(),
                               guards=[])
    before = recipe.state(fixture)
    print(f"== {role} {variant.id}: {run_dir}", flush=True)
    try:
        run.launch()
        for step in scenario.steps_for(spec, variant):
            if "guard" in step:
                guard(run, step)
            else:
                run.run([session_step(step, role)])
                if "capture" in step:
                    require_settled(run, step, role)
    except (SystemExit, Exception) as error:
        record.update(error=str(error) if isinstance(error, SystemExit) else repr(error),
                      refusal=isinstance(error, SystemExit))
        run.log["error"] = record["error"]
    finally:
        code = run.close()
    header = run.log["header"]
    unchanged = bool(run.log.get("fixture_unchanged")) and recipe.state(fixture) == before
    record.update(started_utc=header["started_utc"], ended_utc=header["ended_utc"], exit=run.log.get("exit"),
                  fixture_unchanged=unchanged, captures=len(run.log["captures"]),
                  guards=len(run.log["scenario"]["guards"]))
    if record["error"] is None and code not in (0, -15):
        record["error"] = f"the app exited with {run.log.get('exit')!r}"
    if record["error"] is None and not unchanged:
        record["error"] = "the fixture's state changed during the launch"
    if run.restore_failures:  # a read_only path kept its mode: reported beside whatever else stopped the launch
        record["error"] = "; ".join([*filter(None, [record["error"]]), *run.restore_failures])
    print(f"   {role} {variant.id}: {'ok' if record['error'] is None else record['error']}", flush=True)
    return record


def host(spec: dict, variants, display: str, backend: str) -> dict:
    """The host as the attestation names it, and the tool versions that decide a crop's bytes."""
    name = platform.platform()
    try:
        for line in Path("/etc/os-release").read_text().splitlines():
            if line.startswith("PRETTY_NAME="):
                name = line.partition("=")[2].strip('"')
    except OSError:
        pass
    shell = ""
    if shutil.which("gnome-shell"):
        try:
            found = subprocess.run(["gnome-shell", "--version"], capture_output=True, text=True, timeout=10)
            shell = found.stdout.strip().replace("GNOME Shell", "GNOME")
        except (OSError, subprocess.TimeoutExpired):
            pass
    sizes = sorted({v.text_size for v in variants if v.text_size})
    from PIL import __version__ as pillow

    parts = [name, shell or os.environ.get("XDG_CURRENT_DESKTOP", ""), f"XWayland {display}", "scale factor 1",
             f"window {spec['window'][0]}x{spec['window'][1]}",
             f"{' and '.join(map(str, sizes))} pt" if sizes else ""]
    return dict(description=", ".join(part for part in parts if part), os=name, desktop=shell, display=display,
                scale="1", input=backend, python=platform.python_version(), pillow=pillow,
                zlib=zlib.ZLIB_RUNTIME_VERSION)


def prepare(spec: dict, fixture: Path | None, fixtures: Path) -> tuple[Path, dict | None, dict]:
    """The repository to open, the recipe manifest (None for --fixture) and the run record's fixture entry."""
    if fixture is not None:
        fixture = runenv.absolute(fixture, "fixture")
        state = recipe.state(fixture)
        return fixture, None, dict(path=str(fixture), source="--fixture", state=state,
                                   summary=f"{fixture} (supplied with --fixture; HEAD {state['head'][:7]}, "
                                           f"{state['ref_count']} refs)")
    if spec["fixture"] is None:
        raise SystemExit("refusing: the scenario has no fixture recipe; pass --fixture PATH")
    manifest = recipe.build(spec["fixture"], fixtures)
    print(f"fixture {'reused' if manifest['reused'] else 'built'}: {manifest['repository']}", flush=True)
    return Path(manifest["repository"]), manifest, dict(
        path=manifest["repository"], source="recipe", reused=manifest["reused"],
        recipe_sha256=manifest["recipe_sha256"], summary=recipe.summary(manifest))


def require_unlocked(backend: str) -> None:
    if backend != "mutter":
        return
    from . import mutter

    if mutter.screen_locked() is not False:
        raise SystemExit("refusing: the desktop is locked, or its lock state cannot be read; nothing launched")


def output_dir(out: Path) -> Path:
    out = runenv.check_run_dir(out)
    runenv.check_commit_run_dir(out)
    return out


def failed(launches: list[dict]) -> dict | None:
    return next((record for record in launches if record["error"] is not None), None)


def run(spec_path: Path, builds: dict[str, Path], out: Path, fixture: Path | None = None, input_choice=None,
        display: str = runenv.DEFAULT_DISPLAY, templates: Path | None = None, jobs: int = 4, settle: float = 5.0,
        fixtures: Path = recipe.DEFAULT_FIXTURES) -> int:
    """`qa.py scenario run`: every role x variant, then crops, manifest, analyses and the privacy scan."""
    from . import mutter, privacy

    spec = scenario.load(spec_path)
    out = output_dir(out)
    if set(builds) != set(spec["roles"]):
        raise SystemExit(f"refusing: the scenario's roles are {', '.join(spec['roles'])}; give exactly one "
                         f"--build ROLE=EXE for each (got {', '.join(sorted(builds)) or 'none'})")
    templates = templates or privacy.default_templates()
    if templates is None or not Path(templates).is_dir():
        raise SystemExit(f"refusing: no privacy templates ({privacy.TEMPLATES_ENV}, {privacy.TEMPLATES_DIR} or "
                         "--templates); the crops could not be scanned before commit")
    loaded = privacy.load_templates(Path(templates), anonymous=True)  # unreadable or empty stops here, before a launch
    entries = {role: identity.describe(builds[role]) for role in spec["roles"]}
    problems = identity.problems([entries[role] for role in spec["roles"]], pair=len(entries) == 2)
    if problems:
        raise SystemExit("refusing: " + "; ".join(problems))
    backend = mutter.choose_input(input_choice, mutter.process_argvs())
    repository, manifest, fixture_record = prepare(spec, fixture, fixtures)
    require_unlocked(backend)
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(spec_path, out / "scenario.json")
    if manifest is not None:
        evidence.write_json(out / "fixture-manifest.json", {k: v for k, v in manifest.items() if k != "reused"})
    record = dict(version=1, task=spec["task"], scenario_sha256=spec["sha256"], started_utc=utc(), builds=entries,
                  host=host(spec, spec["variants"], display, backend), fixture=fixture_record, launches=[])
    options = dict(display=display, settle=settle, backend=backend)
    for variant in spec["variants"]:
        for role in spec["roles"]:
            record["launches"].append(launch(spec, role, variant, builds[role], repository,
                                             out / role / variant.id, options))
            evidence.write_json(out / "run.json", record)
            if failed(record["launches"]):
                break
        if failed(record["launches"]):
            break
    stop = failed(record["launches"])
    findings = []
    if stop is None:
        crops = evidence.write_crops(out, scenario.committed(spec), out / evidence.COMMIT, spec["window"])
        evidence.write_json(out / "commit-manifest.json", evidence.commit_manifest(spec, crops))
        analyses = evidence.run_analyses(spec, out)
        evidence.write_json(out / "analysis.json", analyses)
        record["privacy"] = evidence.privacy_scan([out / evidence.COMMIT / c["name"] for c in crops], loaded, jobs)
        findings += analyses["unexpected"]
        findings += [f"privacy: {name} matched a template" for name, verdict in
                     record["privacy"]["verdicts"].items() if verdict != "clean"]
        print(f"{len(crops)} crops in {out / evidence.COMMIT}; analyses {analyses['as_expected']} of "
              f"{analyses['total']} as expected; privacy {record['privacy']['matched']} of "
              f"{record['privacy']['frames']} matched", flush=True)
    else:
        findings.append(f"{stop['role']} {stop['variant']}: {stop['error']}")
    code = 0 if not findings else (2 if stop is not None and stop["refusal"] else 1)
    record["ended_utc"] = utc()
    record["verdict"] = dict(result="pass" if code == 0 else "fail", exit=code, findings=findings)
    evidence.write_json(out / "run.json", record)
    for finding in findings:
        print(f"FINDING: {finding}", flush=True)
    print(f"bundle: {out} ({record['verdict']['result']})", flush=True)
    return code


def recheck(spec_path: Path, exe: Path, committed: Path, out: Path | None = None, fixture: Path | None = None,
            input_choice=None, display: str = runenv.DEFAULT_DISPLAY, settle: float = 5.0,
            fixtures: Path = recipe.DEFAULT_FIXTURES) -> int:
    """`qa.py recheck`: re-capture the candidate crops with `exe` and compare their bytes with the committed ones."""
    from . import mutter

    spec = scenario.load(spec_path)
    crops = scenario.committed(spec, roles=("cand",))
    if not crops:
        raise SystemExit("refusing: the scenario commits no candidate crop to re-check")
    committed = Path(committed).resolve()
    if not committed.is_dir():
        raise SystemExit(f"refusing: {committed} is not a directory of committed crops")
    entry = identity.describe(runenv.absolute(exe, "executable"))
    problems = identity.problems([entry], pair=False)
    if problems:
        raise SystemExit("refusing: " + "; ".join(problems))
    if out is None:
        out = runenv.EVIDENCE_ROOT / "runs" / f"{spec['task']}-recheck-{entry['sha256'][:12]}-" \
                                             f"{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}"
    out = output_dir(out)
    backend = mutter.choose_input(input_choice, mutter.process_argvs())
    repository, manifest, fixture_record = prepare(spec, fixture, fixtures)
    require_unlocked(backend)
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(spec_path, out / "scenario.json")
    variants = [v for v in spec["variants"] if any(crop.variant == v for crop in crops)]
    record = dict(version=1, task=spec["task"], scenario_sha256=spec["sha256"], started_utc=utc(),
                  executable=entry, committed=str(committed), host=host(spec, variants, display, backend),
                  fixture=fixture_record, launches=[])
    options = dict(display=display, settle=settle, backend=backend)
    for variant in variants:
        record["launches"].append(launch(spec, "cand", variant, Path(entry["path"]), repository,
                                         out / "cand" / variant.id, options))
        evidence.write_json(out / "recheck.json", record)
        if failed(record["launches"]):
            break
    stop = failed(record["launches"])
    if stop is None:
        evidence.write_crops(out, crops, out / evidence.COMMIT, spec["window"])
        record.update(evidence.compare_committed(crops, out / evidence.COMMIT, committed))
        code = 0 if record["verdict"] == "identical" else 1
    else:
        record.update(verdict="inconclusive", results=[], error=f"{stop['variant']}: {stop['error']}")
        code = 2 if stop["refusal"] else 1
    record["ended_utc"] = utc()
    evidence.write_json(out / "recheck.json", record)
    for result in record["results"]:
        line = f"{result['name']}: {result['result']}"
        if result.get("differing_pixels"):
            line += f", {result['differing_pixels']} px differ in {result.get('regions', [])[:4]}"
        print(line, flush=True)
    print(f"re-check {record['verdict']}: {out / 'recheck.json'}", flush=True)
    return code
