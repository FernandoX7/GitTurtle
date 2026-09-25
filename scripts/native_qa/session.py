"""One isolated GitTurtle launch: seed, start, find the window by PID, drive a scenario, stop.

The session terminates only the process it started, with SIGTERM. It never
sends SIGKILL and never kills by name, because the operator often runs their
own GitTurtle; a process that outlives SIGTERM is reported and left alone.
"""

from __future__ import annotations

import hashlib
import json
import signal
import subprocess
import sys
import time
from pathlib import Path

from . import identity, runenv

DEFAULT_SCENARIO = [
    {"stable": 4.0},
    {"capture": "00-launch", "what": "after launch, pointer parked"},
]
STEP_KEYS = ("key", "type", "move", "glide", "click", "press", "release", "wheel", "park", "wait",
             "stable", "capture", "resize")


def utc() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def git(fixture: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(fixture), "--no-optional-locks", *args],
                            capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else f"error: {result.stderr.strip()}"


def fixture_state(fixture: Path) -> dict:
    """HEAD, porcelain status and index digest, to show the launch left the fixture unchanged."""
    index = git(fixture, "rev-parse", "--path-format=absolute", "--git-path", "index")
    try:
        index_sha = hashlib.sha256(Path(index).read_bytes()).hexdigest()
    except OSError as error:
        index_sha = f"error: {error}"
    return dict(head=git(fixture, "rev-parse", "HEAD"),
                status=git(fixture, "status", "--porcelain=v1", "--untracked-files=all"),
                index_sha256=index_sha)


def load_scenario(path: Path | None) -> list[dict]:
    steps = DEFAULT_SCENARIO if path is None else json.loads(Path(path).read_text())
    if not isinstance(steps, list):
        raise SystemExit("refusing: a scenario is a JSON list of steps")
    for number, step in enumerate(steps):
        actions = [key for key in STEP_KEYS if key in step] if isinstance(step, dict) else []
        if len(actions) != 1:
            raise SystemExit(f"refusing: scenario step {number} needs exactly one of {', '.join(STEP_KEYS)}: {step!r}")
        if actions == ["wheel"] and "steps" not in step:
            raise SystemExit(f"refusing: scenario step {number} is a wheel without steps: {step!r}")
    return steps


class Session:
    def __init__(self, binary: Path, fixture: Path, run_dir: Path, preferences: bytes, *,
                 width: int = 1000, height: int = 680, display: str = runenv.DEFAULT_DISPLAY,
                 scale: str = runenv.DEFAULT_SCALE, for_commit: bool = False,
                 extra_env: dict[str, str] | None = None, settle: float = 5.0) -> None:
        # Every refusal happens before the run directory is created or the binary is run.
        self.binary = runenv.absolute(binary, "binary")
        self.fixture, warnings = runenv.check_fixture(fixture, for_commit)
        run_dir = runenv.check_run_dir(run_dir)
        if for_commit:
            runenv.check_commit_run_dir(run_dir)
        runenv.check_extra(extra_env or {})
        described = identity.describe(self.binary)
        for warning in warnings:
            print(f"warning: {warning}", file=sys.stderr, flush=True)
        self.dirs = runenv.prepare(run_dir, preferences)
        self.env = runenv.launch_env(self.dirs, display=display, scale=scale, extra=extra_env)
        self.width, self.height, self.settle = width, height, settle
        self.proc = self.driver = None
        self.log = dict(
            header=dict(
                binary=described, fixture=str(self.fixture),
                fixture_before=fixture_state(self.fixture), store_sha256=identity.sha256_bytes(preferences),
                size=[width, height], display=display, scale=scale, backend="XWayland (WAYLAND_DISPLAY unset)",
                for_commit=for_commit, warnings=warnings, argv=sys.argv, started_utc=utc(),
                env={key: self.env[key] for key in (*runenv.LAYOUT, "DISPLAY", "GPUI_X11_SCALE_FACTOR")},
                env_extra=sorted(extra_env or ()),
            ),
            input=[], captures=[], checks=[])
        if described["build_info"].get("source_tree") != "clean":
            print("warning: the binary's source_tree is not clean", file=sys.stderr, flush=True)

    # ---------- lifecycle ----------
    def launch(self) -> None:
        from . import x11

        argv = [str(self.binary), str(self.fixture)]
        self.applog = open(self.dirs.root / "app.log", "wb")
        self.proc = subprocess.Popen(argv, env=self.env, stdout=self.applog, stderr=subprocess.STDOUT)
        self.log["launch"] = dict(argv=argv, pid=self.proc.pid, at_utc=utc())
        print(f"launched pid {self.proc.pid}", flush=True)
        dsp = x11.connect(self.env["DISPLAY"])
        self.driver = x11.Driver(dsp, x11.find_window(dsp, self.proc.pid), self.log["input"])
        self.driver.resize(self.width, self.height)
        time.sleep(self.settle)
        self.driver.activate()
        self.driver.park()

    def close(self, timeout: float = 15.0) -> int | None:
        """SIGTERM to the launched PID only; a survivor is reported, never killed harder."""
        if self.proc is not None:
            if self.proc.poll() is None:
                self.proc.send_signal(signal.SIGTERM)
                try:
                    self.proc.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    self.log["exit"] = f"pid {self.proc.pid} still running {timeout}s after SIGTERM; not killed"
                    print(f"error: {self.log['exit']}", file=sys.stderr, flush=True)
            if "exit" not in self.log:
                self.log["exit"] = self.proc.returncode
            self.applog.close()
        header = self.log["header"]
        header["ended_utc"] = utc()
        header["fixture_after"] = fixture_state(self.fixture)
        self.log["fixture_unchanged"] = header["fixture_before"] == header["fixture_after"]
        try:
            self.log["store_final"] = json.loads(self.dirs.preferences.read_text())
        except (OSError, ValueError) as error:
            self.log["store_final"] = repr(error)
        (self.dirs.root / "flow-log.json").write_text(json.dumps(self.log, indent=1, default=str))
        print(f"fixture HEAD/status/index unchanged: {self.log['fixture_unchanged']}", flush=True)
        return self.log.get("exit") if isinstance(self.log.get("exit"), int) else None

    # ---------- frames ----------
    def capture(self, name: str, what: str = "", park: bool = True, settle: float = 0.0):
        """Grab the window into captures/NAME.png; the pointer is parked first unless the frame is a hover."""
        if park:
            self.driver.park()
        time.sleep(settle)
        frame = self.driver.snap()
        path = self.dirs.captures / f"{name}.png"
        if path.exists():
            raise SystemExit(f"refusing: {path} already exists in this run")
        frame.save(path)
        self.log["captures"].append(dict(capture=path.name, what=what, parked=park, at_utc=utc(),
                                         sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
        print(f"captured {path.name}: {what}", flush=True)
        return frame

    # ---------- scenarios ----------
    def run(self, steps: list[dict]) -> None:
        d = self.driver
        for step in steps:
            note = step.get("note")
            if "key" in step:
                d.key(step["key"], step.get("mods", ()), note, wait=step.get("wait_after", 0.4))
            elif "type" in step:
                d.type(step["type"], note)
            elif "move" in step:
                d.move(*step["move"], note)
            elif "glide" in step:
                d.glide(*step["glide"], note, settle=step.get("settle", 0.8))
            elif "click" in step:
                d.click(*step["click"], note)
            elif "press" in step:
                d.button(True, step["press"], note)
            elif "release" in step:
                d.button(False, step["release"], note)
            elif "wheel" in step:
                d.wheel(*step["wheel"], step["steps"])
            elif "park" in step:
                d.park()
            elif "wait" in step:
                time.sleep(step["wait"])
            elif "stable" in step:
                settled = d.stable(timeout=step["stable"])
                self.log["checks"].append(dict(check="stable", seconds=settled))
            elif "capture" in step:
                self.capture(step["capture"], step.get("what", ""), park=not step.get("keep_pointer", False),
                             settle=step.get("settle", 0.0))
            elif "resize" in step:
                d.resize(*step["resize"])
