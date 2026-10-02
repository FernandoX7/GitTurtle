"""One isolated GitTurtle launch: seed, start, find the window by PID, drive a scenario, stop.

The session terminates only the process it started, with SIGTERM. It never
sends SIGKILL and never kills by name, because the operator often runs their
own GitTurtle; a process that outlives SIGTERM is reported and left alone.
With Mutter input, nothing is sent while the desktop is locked: Mutter
delivers keys to whatever surface has compositor focus, the lock screen's
password field included.
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
STEP_KEYS = ("key", "type", "palette", "move", "glide", "click", "press", "release", "wheel", "park", "wait",
             "stable", "mark", "capture", "resize")
NO_INPUT = ("wait", "stable")  # steps that send nothing, so they need no lock check
PALETTE_KEY = ("p", ("Control_L", "Shift_L"))  # the command palette's shortcut
# The palette is a modal dialog that dims the whole window: opening it changed about 640,000 of 680,000 pixels
# on 2026-10-02 (Midnight and Porcelain, 1000x680), where a caret blink changes under 100.
OVERLAY_SHARE = 0.5


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
                 extra_env: dict[str, str] | None = None, settle: float = 5.0, backend: str = "xtest",
                 lock_check=None) -> None:
        # Every refusal happens before the run directory is created or the binary is run.
        self.binary = runenv.absolute(binary, "binary")
        self.fixture, warnings = runenv.check_fixture(fixture, for_commit)
        run_dir = runenv.check_run_dir(run_dir)
        if for_commit:
            runenv.check_commit_run_dir(run_dir)
        runenv.check_extra(extra_env or {})
        if backend not in ("xtest", "mutter"):
            raise SystemExit(f"refusing: unknown input backend {backend!r}")
        described = identity.describe(self.binary)
        for warning in warnings:
            print(f"warning: {warning}", file=sys.stderr, flush=True)
        self.dirs = runenv.prepare(run_dir, preferences)
        self.env = runenv.launch_env(self.dirs, display=display, scale=scale, extra=extra_env)
        self.width, self.height, self.settle = width, height, settle
        self.input = backend
        if lock_check is None and backend == "mutter":
            from . import mutter

            lock_check = mutter.screen_locked
        self.lock_check = lock_check
        self.frames: dict = {}  # captures and marks by name, for a scenario's guards
        self.proc = self.driver = self.remote = None
        self.log = dict(
            header=dict(
                binary=described, fixture=str(self.fixture),
                fixture_before=fixture_state(self.fixture), store_sha256=identity.sha256_bytes(preferences),
                size=[width, height], display=display, scale=scale, backend="XWayland (WAYLAND_DISPLAY unset)",
                input=backend, for_commit=for_commit, warnings=warnings, argv=sys.argv, started_utc=utc(),
                env={key: self.env[key] for key in (*runenv.LAYOUT, "DISPLAY", "GPUI_X11_SCALE_FACTOR")},
                env_extra=sorted(extra_env or ()),
            ),
            input=[], captures=[], checks=[])
        if described["build_info"].get("source_tree") != "clean":
            print("warning: the binary's source_tree is not clean", file=sys.stderr, flush=True)

    # ---------- lifecycle ----------
    def require_unlocked(self, why: str) -> None:
        """Refuse input unless the desktop reports itself unlocked (Mutter input only)."""
        if self.lock_check is None:
            return
        locked = self.lock_check()
        if locked is not False:
            state = "locked" if locked else "of unknown lock state (org.gnome.ScreenSaver.GetActive unreadable)"
            raise runenv.Refusal(f"the desktop is {state} before {why}; nothing sent")

    def launch(self) -> None:
        from . import x11

        self.require_unlocked("the launch")
        argv = [str(self.binary), str(self.fixture)]
        self.applog = open(self.dirs.root / "app.log", "wb")
        self.proc = subprocess.Popen(argv, env=self.env, stdout=self.applog, stderr=subprocess.STDOUT)
        self.log["launch"] = dict(argv=argv, pid=self.proc.pid, at_utc=utc(), input=self.input)
        print(f"launched pid {self.proc.pid} (input {self.input})", flush=True)
        dsp = x11.connect(self.env["DISPLAY"])
        window = x11.find_window(dsp, self.proc.pid)
        if self.input == "mutter":
            from . import mutter

            self.remote = mutter.RemoteDesktop.connect()
            self.driver = mutter.MutterDriver(dsp, window, self.log["input"], self.remote)
        else:
            self.driver = x11.Driver(dsp, window, self.log["input"])
        self.driver.resize(self.width, self.height)
        time.sleep(self.settle)
        self.driver.activate()
        self.driver.park()

    def close(self, timeout: float = 15.0) -> int | None:
        """SIGTERM to the launched PID only; a survivor is reported, never killed harder."""
        if self.remote is not None:
            self.remote.stop()
            self.remote = None
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
    def capture(self, name: str, what: str = "", park: bool = True, settle: float = 0.0,
                stable: float | None = None, quiet: float = 1.15):
        """Grab the window into captures/NAME.png; the pointer is parked first unless the frame is a hover.

        With `stable`, wait up to that many seconds (after parking) for two
        identical grabs `quiet` apart; a window that does not settle is
        recorded, not refused.
        """
        path = self.dirs.captures / f"{name}.png"
        if path.exists():
            raise SystemExit(f"refusing: {path} already exists in this run")
        if park:
            self.driver.park()
        settled = self.driver.stable(timeout=stable, quiet=quiet) if stable else None
        time.sleep(settle)
        frame = self.driver.snap()
        frame.save(path)
        self.frames[name] = frame
        self.log["captures"].append(dict(capture=path.name, what=what, parked=park, at_utc=utc(), stable_s=settled,
                                         sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
        unsettled = " (window not settled)" if stable and settled is None else ""
        print(f"captured {path.name}: {what}{unsettled}", flush=True)
        return frame

    def mark(self, name: str, park: bool = False, stable: float | None = None, quiet: float = 0.5):
        """Keep the current frame as `name` for later guards, saved under marks/ (never committed)."""
        if park:
            self.driver.park()
        if stable:
            self.driver.stable(timeout=stable, quiet=quiet)
        frame = self.driver.snap()
        self.frames[name] = frame
        marks = self.dirs.root / "marks"
        marks.mkdir(exist_ok=True)
        frame.save(marks / f"{len(list(marks.iterdir())):02d}-{name}.png")
        self.log["checks"].append(dict(check="mark", name=name, at_utc=utc()))
        return frame

    def press_key(self, name: str, mods, note: str | None, wait: float, await_change: float | None) -> None:
        """One key press; with `await_change`, the seconds until the window changed are logged (None: it did not)."""
        if await_change is None:
            self.driver.key(name, mods, note, wait=wait)
            return
        before = self.driver.snap()
        self.driver.key(name, mods, note, wait=0.0)
        latency, _ = self.driver.wait_change(before, timeout=await_change)
        self.log["checks"].append(dict(check=f"key {name} first change", seconds=latency))
        time.sleep(wait)

    def wait_overlay(self, before, timeout: float, share: float = OVERLAY_SHARE) -> float | None:
        """Seconds until at least `share` of the window differs from `before`, as when a modal dialog dims it.

        A caret blink or a spinner changes a few hundred pixels at most, so it never counts.
        """
        from . import frames

        start = time.perf_counter()
        needed = share * before.width * before.height
        while time.perf_counter() - start < timeout:
            frame = self.driver.snap()
            if frame.size == before.size and frames.difference_mask(before, frame).histogram()[255] >= needed:
                return round(time.perf_counter() - start, 3)
            time.sleep(0.05)
        return None

    def palette(self, query: str, note: str | None = None) -> None:
        """Run a command-palette entry by its visible name.

        Stops before typing unless the palette's modal dialog visibly covered
        the window (`wait_overlay`), and after Return unless the window
        changed, so a missed shortcut never types the query or presses Return
        into whatever else has focus.
        """
        d = self.driver
        before = d.snap()
        d.key(PALETTE_KEY[0], PALETTE_KEY[1], note or "command palette", wait=0.0)
        opened = self.wait_overlay(before, timeout=3.0)
        if opened is None:
            raise runenv.Refusal(f"the command palette did not open for {query!r}; nothing typed")
        time.sleep(0.4)
        self.require_unlocked(f"typing {query!r}")
        d.type(query, "palette query")
        d.stable(timeout=4.0, quiet=0.6)
        typed = d.snap()
        self.require_unlocked(f"Return on {query!r}")
        d.key("Return", (), f"run {query!r}", wait=0.0)
        ran, _ = d.wait_change(typed, timeout=5.0)
        settled = d.stable(timeout=10.0, quiet=1.15)
        self.log["checks"].append(dict(check=f"palette {query!r}", opened_s=opened, ran_s=ran, settled_s=settled))
        if ran is None:
            raise runenv.Refusal(f"the palette command {query!r} changed nothing within 5 s; stopping")

    # ---------- scenarios ----------
    def run(self, steps: list[dict]) -> None:
        d = self.driver
        for step in steps:
            note = step.get("note")
            if not any(key in step for key in NO_INPUT):
                self.require_unlocked(f"step {next((key for key in STEP_KEYS if key in step), '?')}")
            if "key" in step:
                for press in range(step.get("repeat", 1)):
                    if press:
                        self.require_unlocked(f"key {step['key']} press {press + 1}")
                    self.press_key(step["key"], step.get("mods", ()), note, step.get("wait_after", 0.4),
                                   step.get("await_change"))
            elif "palette" in step:
                self.palette(step["palette"], note)
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
                settled = d.stable(timeout=step["stable"], quiet=step.get("quiet", 0.5))
                self.log["checks"].append(dict(check="stable", seconds=settled))
            elif "mark" in step:
                self.mark(step["mark"], step.get("park_first", False), step.get("stable_within"),
                          step.get("quiet", 0.5))
            elif "capture" in step:
                self.capture(step["capture"], step.get("what", ""), park=not step.get("keep_pointer", False),
                             settle=step.get("settle", 0.0), stable=step.get("stable_within"),
                             quiet=step.get("quiet", 1.15))
            elif "resize" in step:
                d.resize(*step["resize"])
