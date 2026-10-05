"""One isolated GitTurtle launch: seed, start, find the window by PID, drive a scenario, stop.

The session terminates only the process it started, with SIGTERM. It never
sends SIGKILL and never kills by name, because the operator often runs their
own GitTurtle; a process that outlives SIGTERM is reported and left alone.
With Mutter input, nothing is sent while the desktop is locked: Mutter
delivers keys to whatever surface has compositor focus, the lock screen's
password field included. A `read_only` step's paths get their modes back in
`close`, whatever ended the launch, before anything else reads or removes the
run directory. `atspi_focus` and `store_snapshot` steps send nothing: they keep
the focused AT-SPI node, or a store's digest, size and JSON, under a label in
flow-log.json's `readings`. A `probe` keeps every distinct frame of a region
drawn after a key or a click under `probes/`, never `captures/`, so nothing it
grabs is committed. Every park, with each pointer motion it sent, goes into
flow-log.json's `parks`; a park never moves the pointer onto the window after
keyboard input (`x11.plan_park`), and a capture taken once one had to is
marked with a `warning`. With `window_minimum`, the window's WM_NORMAL_HINTS
minimum is lowered and read back before the first resize, and flow-log.json's
`window_minimum` keeps the hints before and after; without it, nothing reads
or writes them.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import signal
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

from . import a11y, identity, runenv, stores

DEFAULT_SCENARIO = [
    {"stable": 4.0},
    {"capture": "00-launch", "what": "after launch, pointer parked"},
]
STEP_KEYS = ("key", "type", "palette", "move", "glide", "click", "press", "release", "wheel", "park", "wait",
             "stable", "mark", "capture", "resize", "read_only", "probe")
NO_INPUT = ("wait", "stable", "read_only")  # steps that send nothing, so they need no lock check
# Steps that read the app's state into flow-log.json's `readings` under a label; they send nothing either.
READING_STEPS = ("atspi_focus", "store_snapshot")
STEP_KEYS += READING_STEPS
NO_INPUT += READING_STEPS
PALETTE_KEY = ("p", ("Control_L", "Shift_L"))  # the command palette's shortcut
# The palette is a modal dialog that dims the whole window: opening it changed about 640,000 of 680,000 pixels
# on 2026-10-02 (Midnight and Porcelain, 1000x680), where a caret blink changes under 100.
OVERLAY_SHARE = 0.5
PROBE_QUIET = 0.3     # s a probed region stays unchanged after a press before it counts as settled
PROBE_TIMEOUT = 3.0   # s after a press before a probe gives up waiting for its region to change and settle
PROBE_STABLE = 4.0    # s a probe waits, before its first press, for the region to stay unchanged `quiet` s
PROBE_FRAME_CAP = 60  # distinct frames kept per press; more ends the press, truncated, so memory stays bounded
# Raw frame bytes kept per press: 98 whole 1000x680 BGRX frames, or 8 of a 3840x2160 window, so name a region.
PROBE_BYTE_BUDGET = 256 * 1024 * 1024
KEYBOARD_MODE_LOST = ("a park had to move the pointer onto the app's window after keyboard input, so the app left "
                      "keyboard mode: indicators it draws only in keyboard mode, such as focus-visible rings, are "
                      "missing from this frame")


def ms(seconds: float) -> float:
    return round(seconds * 1000, 2)


def interval_stats(gaps: list[float]) -> dict | None:
    """Min, median and max of the seconds between consecutive grabs, in ms (None without two grabs)."""
    if not gaps:
        return None
    return dict(min=ms(min(gaps)), median=ms(statistics.median(gaps)), max=ms(max(gaps)))


def wait_quiet(grab, timeout: float, quiet: float, clock=time.perf_counter, pause=time.sleep,
               every: float = 0.01) -> float | None:
    """Seconds until two grabs `quiet` s apart, and every grab between them, were identical; None after `timeout`
    (counted, like `quiet`, from the first grab's return, so a `timeout` of at least `quiet` can settle)."""
    last = grab()
    start = since = clock()
    while True:
        now = clock()
        if now - since >= quiet:
            return round(now - start, 3)
        if now - start >= timeout:
            return None
        pause(every)
        data = grab()
        if data != last:
            last, since = data, clock()


def probe_press(grab, send, quiet: float, timeout: float, cap: int = PROBE_FRAME_CAP,
                clock=time.perf_counter, ready=None,
                budget: int = PROBE_BYTE_BUDGET) -> tuple[dict, list[bytes], list[float]]:
    """One press of a probe: the region just before `send()`, then grabs back to back until it has been unchanged
    for `quiet` s after a change (`ended` "quiet"), `timeout` s passed since the press ("timeout"; a press that
    changed nothing ends so), `cap` distinct frames were kept ("frame-cap") or their raw bytes reached `budget`
    ("byte-budget"). The last two leave the press `truncated`: its frames stop before the region settled.

    `ready()` runs between that first grab and `send()`, for the checks that
    must come immediately before the press; if it raises, nothing is sent.
    Only the raw bytes are compared while grabbing; nothing is decoded or
    written until the press ends. Times are ms since `send()` returned. Each
    kept frame records `ms`, when the grab that first showed it returned,
    `previous_ms`, when the grab before it returned (it still showed the
    earlier frame), `last_ms` and `grabs`, the last grab and the number of
    grabs that showed it. Frame 0 is the region grabbed just before the
    press, at a negative `ms`; its `last_ms` is the last grab before the first
    change. `resolution_ms` is the longest time without a grab, from that
    grab to the first after the press and between grabs: a frame shown for
    less may have been missed. Returns the press's record, the kept frames'
    bytes and the seconds between consecutive grabs.
    """
    before = grab()
    grabbed = clock()
    if ready is not None:
        ready()
    started = clock()
    send()
    pressed = clock()
    kept = [dict(index=0, ms=ms(grabbed - pressed), previous_ms=None, last_ms=ms(grabbed - pressed), grabs=1)]
    raw, times, held = [before], [], len(before)
    last, changed_at, ended = before, None, "timeout"
    while True:
        data = grab()
        now = clock() - pressed
        times.append(now)
        if data != last:
            kept.append(dict(index=len(kept), ms=ms(now),
                             previous_ms=ms(times[-2] if len(times) > 1 else grabbed - pressed),
                             last_ms=ms(now), grabs=1))
            raw.append(data)
            held += len(data)
            last, changed_at = data, now
            if len(kept) - 1 >= cap:
                ended = "frame-cap"
                break
            if held >= budget:
                ended = "byte-budget"
                break
        else:
            kept[-1]["grabs"] += 1
            kept[-1]["last_ms"] = ms(now)
            if changed_at is not None and now - changed_at >= quiet:
                ended = "quiet"
                break
        if now >= timeout:
            break
    gaps = [b - a for a, b in zip(times, times[1:])]
    record = dict(send_ms=ms(pressed - started), grabs=len(times), first_grab_ms=ms(times[0]),
                  interval_ms=interval_stats(gaps),
                  resolution_ms=ms(max([times[0] + pressed - grabbed, *gaps])),
                  first_change_ms=kept[1]["ms"] if len(kept) > 1 else None,
                  last_change_ms=None if changed_at is None else ms(changed_at), end_ms=ms(times[-1]),
                  ended=ended, changed=len(kept) > 1, settled=ended == "quiet",
                  truncated=ended in ("frame-cap", "byte-budget"), bytes=held, frames=kept)
    return record, raw, gaps


class PointerMoved(Exception):
    """The pointer was no longer at a click probe's point on the app window just before its press."""


def probe_input_problem(send, click_at, release_at) -> str | None:
    """Why a probe's input is not exactly one key (`send`) or one pointer press at `click_at`, or None."""
    if (send is None) == (click_at is None):
        return "a probe sends a key (\"send\") or presses a point (\"click_at\"), exactly one of them"
    if release_at is not None and click_at is None:
        return "\"release_at\" moves a probe's pressed button elsewhere before releasing it, so it needs \"click_at\""
    return None


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


@contextlib.contextmanager
def sigterm_deferred():
    """Keep SIGTERM pending through a section that must finish; it is delivered on leaving, once state is consistent.

    Main thread only, like the handler `Session.watch_sigterm` installs. A mask
    is per thread, so this relies on the main thread being the only thread
    that can take the signal during a launch: none other runs until the bundle
    is scanned. The sections start no process, so no child inherits the mask.
    """
    if threading.current_thread() is not threading.main_thread():
        yield
        return
    previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
    try:
        yield
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)


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
        if actions == ["probe"]:
            problem = probe_input_problem(step.get("send"), step.get("click_at"), step.get("release_at"))
            if problem is not None:
                raise SystemExit(f"refusing: scenario step {number}: {problem}: {step!r}")
        if actions == ["read_only"]:
            problem = runenv.read_only_problem(step["read_only"])
            if problem is not None:
                raise SystemExit(f"refusing: scenario step {number} ($[{number}].read_only): {problem}")
        if actions == ["store_snapshot"] and "path" in step:
            problem = runenv.read_only_problem(step["path"])  # a run-directory path in its XDG homes
            if problem is not None:
                raise SystemExit(f"refusing: scenario step {number} ($[{number}].path): {problem}")
    return steps


class Session:
    def __init__(self, binary: Path, fixture: Path, run_dir: Path, preferences: bytes, *,
                 width: int = 1000, height: int = 680, display: str = runenv.DEFAULT_DISPLAY,
                 scale: str = runenv.DEFAULT_SCALE, for_commit: bool = False,
                 extra_env: dict[str, str] | None = None, settle: float = 5.0, backend: str = "xtest",
                 lock_check=None, home_files: dict[str, bytes] | None = None,
                 window_minimum: tuple[int, int] | None = None,
                 app_config_files: dict[str, bytes] | None = None) -> None:
        # Every refusal happens before the run directory is created or the binary is run.
        self.binary = runenv.absolute(binary, "binary")
        self.fixture, warnings = runenv.check_fixture(fixture, for_commit)
        run_dir = runenv.check_run_dir(run_dir)
        if for_commit:
            runenv.check_commit_run_dir(run_dir)
        runenv.check_extra(extra_env or {})
        home_files = runenv.check_home_files(home_files or {})
        app_config_files = runenv.check_app_config_files(app_config_files or {})
        if backend not in ("xtest", "mutter"):
            raise SystemExit(f"refusing: unknown input backend {backend!r}")
        described = identity.describe(self.binary)
        for warning in warnings:
            print(f"warning: {warning}", file=sys.stderr, flush=True)
        self.dirs = runenv.prepare(run_dir, preferences, home_files=home_files, app_config_files=app_config_files)
        self.env = runenv.launch_env(self.dirs, display=display, scale=scale, extra=extra_env)
        self.width, self.height, self.settle = width, height, settle
        self.window_minimum = None if window_minimum is None else tuple(window_minimum)
        self.input = backend
        if lock_check is None and backend == "mutter":
            from . import mutter

            lock_check = mutter.screen_locked
        self.lock_check = lock_check
        self.frames: dict = {}  # captures and marks by name, for a scenario's guards
        self.readings: dict[str, dict] = {}  # atspi_focus and store_snapshot readings by label, for guards
        self.clock, self.pause = time.perf_counter, time.sleep  # a probe's; the tests drive them
        self.proc = self.driver = self.remote = None
        self.locked: list[tuple[runenv.Locked, dict]] = []  # read_only paths, restored last first by close
        self.restore_failures: list[str] = []
        self.watching_sigterm, self.previous_sigterm = False, None
        self.log = dict(
            header=dict(
                binary=described, fixture=str(self.fixture),
                fixture_before=fixture_state(self.fixture), store_sha256=identity.sha256_bytes(preferences),
                home_files={path: identity.sha256_bytes(data) for path, data in sorted(home_files.items())},
                app_config_files={name: identity.sha256_bytes(data) for name, data in sorted(app_config_files.items())},
                size=[width, height], display=display, scale=scale, backend="XWayland (WAYLAND_DISPLAY unset)",
                input=backend, for_commit=for_commit, warnings=warnings, argv=sys.argv, started_utc=utc(),
                env={key: self.env[key] for key in (*runenv.LAYOUT, "DISPLAY", "GPUI_X11_SCALE_FACTOR")},
                env_extra=sorted(extra_env or ()),
            ),
            input=[], parks=[], captures=[], checks=[], read_only=[], readings=[], probes=[])
        if self.window_minimum is not None:  # only then, so a launch without it logs exactly what it did before
            self.log["header"]["window_minimum"] = list(self.window_minimum)
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
        self.size_window()
        time.sleep(self.settle)
        self.driver.activate()
        self.park("launch")

    def size_window(self) -> None:
        """Resize the found window to the launch size, first lowering its WM_NORMAL_HINTS minimum to
        `window_minimum` when one is given (`x11.Driver.lower_minimum`), with the hints before and after it in
        flow-log.json's `window_minimum`. A minimum that lowers nothing or does not read back refuses before any
        resize, and the hints it read, with the reason under `refused`, are logged all the same."""
        if self.window_minimum is not None:
            record = self.log["window_minimum"] = {}  # in the log first: `close` writes it even after a refusal
            self.driver.lower_minimum(*self.window_minimum, record=record)
            was = record["original"]["min_size"]
            print(f"window minimum lowered from {was[0]}x{was[1]} to {self.window_minimum[0]}x"
                  f"{self.window_minimum[1]} (WM_NORMAL_HINTS, read back)", flush=True)
        self.driver.resize(self.width, self.height)

    def close(self, timeout: float = 15.0) -> int | None:
        """SIGTERM to the launched PID only; a survivor is reported, never killed harder.

        Every `read_only` path gets its mode back here, even when stopping the
        app fails; a mode that cannot be restored is printed and kept in
        `restore_failures`, which fails the launch.
        """
        try:
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
        finally:
            self.restore_read_only()
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

    # ---------- read-only paths ----------
    def watch_sigterm(self) -> None:
        """While a path is read-only, SIGTERM to this tool ends the launch through `close`, which restores it."""
        if self.watching_sigterm or threading.current_thread() is not threading.main_thread():
            return

        def stop(signum, frame):
            signal.signal(signal.SIGTERM, signal.SIG_IGN)  # a second SIGTERM must not interrupt the restore
            raise SystemExit("stopped: SIGTERM received")

        self.previous_sigterm = signal.signal(signal.SIGTERM, stop)
        self.watching_sigterm = True

    def read_only(self, relative: str, note: str | None = None) -> None:
        """Remove the write bits of a run-directory path until `close` (a `read_only` step); it sends no input."""
        problem = runenv.read_only_problem(relative)
        if problem is not None:
            raise runenv.Refusal(f"read_only: {problem}")
        target, fixture = (self.dirs.root / relative).resolve(), self.fixture.resolve()
        if runenv.within(target, fixture) or runenv.within(fixture, target):
            raise runenv.Refusal(f"read_only {relative}: {target} overlaps the fixture {fixture}; nothing changed")
        self.watch_sigterm()
        with sigterm_deferred():  # a changed mode is always recorded for close before SIGTERM can end the launch
            locked = runenv.lock_read_only(self.dirs.root, relative)
            entry = dict(path=relative, old_mode=f"{locked.old_mode:04o}", new_mode=f"{locked.new_mode:04o}",
                         at_utc=utc(), restored=None)
            self.locked.append((locked, entry))
            self.log["read_only"].append(entry)
            self.log["input"].append(f"read_only {relative}: mode {entry['old_mode']} -> {entry['new_mode']}"
                                     + (f"  # {note}" if note else ""))
        print(f"read_only {relative}: mode {entry['old_mode']} -> {entry['new_mode']} until the launch ends",
              flush=True)

    def restore_read_only(self) -> None:
        """Put back every mode `read_only` removed, the last first; a failure is printed and kept, never dropped.

        A SIGTERM that arrives meanwhile waits until every mode is back and the
        tool's own handler is reinstated, which then receives it.
        """
        with sigterm_deferred():
            while self.locked:
                locked, entry = self.locked.pop()
                try:
                    runenv.restore_mode(locked)
                except OSError as error:
                    entry.update(restored=False, error=str(error))
                    message = (f"could not restore mode {entry['old_mode']} of {locked.path} in {self.dirs.root}: "
                               f"{error}")
                    self.restore_failures.append(message)
                    print(f"error: {message}", file=sys.stderr, flush=True)
                else:
                    entry["restored"] = True
                    self.log["input"].append(f"restore {locked.path}: mode {entry['new_mode']} -> {entry['old_mode']}")
            if self.watching_sigterm:
                signal.signal(signal.SIGTERM, signal.SIG_DFL if self.previous_sigterm is None else self.previous_sigterm)
                self.watching_sigterm = False

    # ---------- pointer ----------
    def park(self, why: str) -> dict:
        """Park the pointer through the driver and keep what it sent, every motion included, in `parks`."""
        record = dict(why=why, at_utc=utc(), **(self.driver.park() or {}))
        self.log["parks"].append(record)
        if record.get("entered_window") and record.get("keyboard_mode"):
            print(f"WARNING: the park for {why} moved the pointer onto the app's window after keyboard input, so the "
                  "app left keyboard mode", file=sys.stderr, flush=True)
        return record

    def keyboard_mode_lost(self) -> bool:
        """True while the app is out of keyboard mode only because a park moved the pointer onto its window."""
        return getattr(self.driver, "keyboard_mode_lost", False) is True

    # ---------- frames ----------
    def capture(self, name: str, what: str = "", park: bool = True, settle: float = 0.0,
                stable: float | None = None, quiet: float = 1.15):
        """Grab the window into captures/NAME.png; the pointer is parked first unless the frame is a hover.

        With `stable`, wait up to that many seconds (after parking) for two
        identical grabs `quiet` apart; a window that does not settle is
        recorded, not refused. Parking keeps the app's keyboard mode; a
        frame taken after a park had to leave it is recorded with a
        `warning` (KEYBOARD_MODE_LOST) and printed as one.
        """
        path = self.dirs.captures / f"{name}.png"
        if path.exists():
            raise SystemExit(f"refusing: {path} already exists in this run")
        parked = self.park(f"capture {name}") if park else {}
        lost = self.keyboard_mode_lost()
        settled = self.driver.stable(timeout=stable, quiet=quiet) if stable else None
        time.sleep(settle)
        frame = self.driver.snap()
        frame.save(path)
        self.frames[name] = frame
        entry = dict(capture=path.name, what=what, parked=park, park_entered_window=bool(parked.get("entered_window")),
                     at_utc=utc(), stable_s=settled, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        if lost:
            entry["warning"] = KEYBOARD_MODE_LOST
            print(f"WARNING: capture {path.name}: {KEYBOARD_MODE_LOST}", file=sys.stderr, flush=True)
        self.log["captures"].append(entry)
        unsettled = " (window not settled)" if stable and settled is None else ""
        print(f"captured {path.name}: {what}{unsettled}", flush=True)
        return frame

    def mark(self, name: str, park: bool = False, stable: float | None = None, quiet: float = 0.5):
        """Keep the current frame as `name` for later guards, saved under marks/ (never committed)."""
        if park:
            self.park(f"mark {name}")
        if stable:
            self.driver.stable(timeout=stable, quiet=quiet)
        frame = self.driver.snap()
        self.frames[name] = frame
        marks = self.dirs.root / "marks"
        marks.mkdir(exist_ok=True)
        frame.save(marks / f"{len(list(marks.iterdir())):02d}-{name}.png")
        self.log["checks"].append(dict(check="mark", name=name, at_utc=utc()))
        return frame

    def probe(self, name: str, send: str | None = None, mods=(), note: str | None = None, region=None,
              repeat: int = 1, quiet: float = PROBE_QUIET, timeout: float = PROBE_TIMEOUT,
              stable_within: float = PROBE_STABLE, keep_pointer: bool = False, click_at=None,
              release_at=None) -> dict:
        """Press the key `send`, or the pointer's first button at `click_at`, `repeat` times and keep every
        distinct frame of `region` drawn after each press.

        A key probe parks the pointer first unless `keep_pointer` (a focused
        and hovered row stays hovered), then gives the region up to
        `stable_within` s to stay unchanged `quiet` s; each press is checked
        for the lock and sent through the driver's `key`, with its focus guard.
        A click probe instead aims the pointer at `click_at` before every
        press through the driver's `aim`, the click steps' guard (XWayland must
        report the pointer there on the app window), and waits for the region
        again, since the hover changes it. Immediately before the button goes
        down the lock is checked and the pointer must still be there; if it
        moved during the wait, it is aimed once more, and a second miss
        refuses with no button sent. Its press sends the button down and
        up back to back; with `release_at`, only down, and once the frames are
        grabbed the pointer is aimed at `release_at` and the button released
        there, so a press can focus a row without the release choosing it.
        From the moment the button goes down its release is owed: whatever
        stops the probe, the release is still sent if the lock check allows,
        and `owed_release` records whether it was.
        `probe_press` grabs only the region, back to back. The frames are
        written after each press to probes/NAME/PRESS-INDEX.png, with
        probe.json and the same record in flow-log.json; none is committed.
        """
        from . import x11

        problem = probe_input_problem(send, click_at, release_at)
        if problem is not None:
            raise SystemExit(f"refusing: probe {name}: {problem}")
        directory = self.dirs.root / "probes" / name
        if directory.exists():
            raise SystemExit(f"refusing: probe {name} already exists in this run")
        width, height = self.driver.size()
        box = tuple(region) if region is not None else (0, 0, width, height)
        if not (0 <= box[0] < box[2] <= width and 0 <= box[1] < box[3] <= height):
            raise runenv.Refusal(f"probe {name}: region {list(box)} is not inside the {width}x{height} window; "
                                 "nothing sent")
        for x, y in (p for p in (click_at, release_at) if p is not None):
            if not (0 <= x < width and 0 <= y < height):
                raise runenv.Refusal(f"probe {name}: point [{x}, {y}] is not inside the {width}x{height} window; "
                                     "nothing sent")
        clicks = click_at is not None
        parked = not clicks and not keep_pointer
        if parked:
            self.park(f"probe {name}")

        def grab():
            return self.driver.grab(box)

        def settle():
            return wait_quiet(grab, stable_within, quiet, self.clock, self.pause) if stable_within else None

        state = dict(press=0, held=None)  # `held`: the press whose button went down and is owed its release

        def ready():
            """Immediately before the press, after any settling: the lock, and the pointer still at `click_at`."""
            self.require_unlocked(f"probe {name} press {state['press']}")
            if clicks and not self.driver.on_window(*click_at):
                raise PointerMoved()

        def send_input():
            if not clicks:
                self.driver.key(send, mods, note, wait=0.0)
                return
            state["held"] = state["press"]  # owed from here on, whatever raises next
            self.driver.button(True, 1, note)
            if release_at is None:
                self.driver.button(False, 1, note)
                state["held"] = None

        def release(why: str) -> None:
            self.require_unlocked(f"probe {name} release {state['held']}"
                                  + (f" at {list(release_at)}" if release_at is not None else ""))
            if release_at is not None:
                self.driver.aim(*release_at, why, settle=0.2)
            self.driver.button(False, 1, note)
            state["held"] = None

        settled = None if clicks else settle()
        directory.mkdir(parents=True)
        size = (box[2] - box[0], box[3] - box[1])
        record = dict(probe=name, key=send, mods=list(mods), click_at=list(click_at) if clicks else None,
                      release_at=list(release_at) if release_at is not None else None, region=list(box),
                      repeat=repeat, quiet=quiet, timeout=timeout, stable_within=stable_within, parked=parked,
                      settled_before_s=settled, at_utc=utc(), presses=[])
        gaps: list[float] = []
        try:
            for press in range(1, repeat + 1):
                state["press"] = press
                before: dict = {}
                for attempt in (1, 2):
                    if clicks:
                        self.require_unlocked(f"probe {name} pointer to {list(click_at)} for press {press}")
                        self.driver.aim(*click_at, note)  # refuses, with nothing pressed, if it lands elsewhere
                        before["settled_before_s"] = settle()
                    try:
                        entry, raw, between = probe_press(grab, send_input, quiet, timeout, clock=self.clock,
                                                          ready=ready)
                        break
                    except PointerMoved:
                        if attempt == 2:
                            raise runenv.Refusal(f"probe {name}: the pointer left {list(click_at)} on the app window "
                                                 f"again before press {press}; no button sent") from None
                        before["reaimed"] = True
                        self.log["input"].append(f"probe {name}: the pointer left {list(click_at)} before press "
                                                 f"{press}; aiming again")
                if clicks and press == 1:
                    record["settled_before_s"] = before["settled_before_s"]
                for frame, data in zip(entry["frames"], raw):
                    path = directory / f"{press:03d}-{frame['index']:02d}.png"
                    x11.image_from(data, size).save(path)
                    frame.update(file=path.name, sha256=identity.sha256_file(path))
                record["presses"].append(dict(press=press, **before, **entry))
                gaps += between
                if release_at is not None:
                    record["presses"][-1]["released"] = False  # until the release below is sent
                    release("probe release point")
                    record["presses"][-1]["released"] = True
        finally:  # a refusal or error mid-probe still leaves the presses it made, and any owed release, on record
            if state["held"] is not None:  # the button went down and was not released: release it, or say so
                owed = state["held"]
                try:
                    release("probe release point, after an error")
                    record["owed_release"] = dict(press=owed, released=True)
                except (Exception, SystemExit) as error:
                    record["owed_release"] = dict(press=owed, released=False, error=str(error))
                    print(f"error: probe {name}: the button pressed at {list(click_at)} for press {owed} was never "
                          f"released: {error}", file=sys.stderr, flush=True)
                for entry_ in record["presses"]:
                    if entry_["press"] == owed:
                        entry_["released"] = record["owed_release"]["released"]
            presses = record["presses"]
            record.update(grabs=sum(p["grabs"] for p in presses), frames=sum(len(p["frames"]) - 1 for p in presses),
                          interval_ms=interval_stats(gaps),
                          resolution_ms=max((p["resolution_ms"] for p in presses), default=None))
            (directory / "probe.json").write_text(json.dumps(record, indent=1) + "\n")
            self.log["probes"].append(record)
        unsettled = sum(1 for p in record["presses"] if not p["settled"])
        print(f"probe {name}: {len(record['presses'])} press(es), {record['frames']} frames after them, "
              f"{record['grabs']} grabs {record['interval_ms']} ms apart, resolution {record['resolution_ms']} ms"
              + (f", {unsettled} unsettled" if unsettled else ""), flush=True)
        return record

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

    # ---------- readings ----------
    def add_reading(self, label: str, kind: str, note: str | None, found: dict) -> dict:
        """Keep a reading under `label` for later guards and log it in flow-log.json's `readings`."""
        if label in self.readings:
            raise SystemExit(f"refusing: reading {label} was already taken in this run")
        entry = dict(label=label, kind=kind, at_utc=utc(), **({"note": note} if note else {}), **found)
        self.readings[label] = entry
        self.log["readings"].append(entry)
        return entry

    def atspi_focus(self, label: str, within: float = a11y.WITHIN, note: str | None = None) -> dict:
        """Read the focused AT-SPI node of the launched process (an `atspi_focus` step); it sends no input."""
        if self.proc is None:
            raise runenv.Refusal(f"atspi_focus {label}: the app is not running")
        try:
            found = a11y.read_focus(self.proc.pid, within)
        except Exception as error:  # the tool could not read AT-SPI: inconclusive, never a finding on the build
            found = dict(pid=self.proc.pid, application=None, focused=None, all_focused=[],
                         error=f"AT-SPI could not be read ({error!r})")
        entry = self.add_reading(label, "atspi_focus", note, found)
        if entry.get("error"):
            raise runenv.Refusal(f"atspi_focus {label}: {entry['error']}")
        focused = entry["focused"]
        shown = "no focused node" if focused is None else \
            f"{focused['name']!r} ({focused['role']}) {', '.join(focused['states'])}"
        print(f"atspi_focus {label}: {shown}", flush=True)
        return entry

    def store_snapshot(self, label: str, relative: str = stores.PREFERENCES, quiet: float = stores.SNAPSHOT_QUIET,
                       within: float = stores.SNAPSHOT_WITHIN, note: str | None = None) -> dict:
        """Wait until a store in the run directory stops changing, then keep its sha256, size and JSON (a
        `store_snapshot` step); one that keeps changing for `within` seconds stops the launch as inconclusive."""
        problem = runenv.read_only_problem(relative)
        if problem is not None:
            raise runenv.Refusal(f"store_snapshot {label}: {problem}")
        try:
            found = stores.snapshot(self.dirs.root, relative, quiet=quiet, within=within)
        except OSError as error:
            raise runenv.Refusal(f"store_snapshot {label}: {error}") from None
        entry = self.add_reading(label, "store_snapshot", note, dict(path=relative, **found))
        if not entry["stable"]:
            raise runenv.Refusal(f"store_snapshot {label}: {relative} was still changing after {within} s")
        shown = f"sha256 {entry['sha256'][:12]}, {entry['bytes']} bytes" if entry["exists"] else "absent"
        print(f"store_snapshot {label}: {relative} {shown} (settled after {entry['waited_s']} s)", flush=True)
        return entry

    def reading_step(self, step: dict) -> None:
        if "atspi_focus" in step:
            self.atspi_focus(step["atspi_focus"], step.get("within", a11y.WITHIN), step.get("note"))
        else:
            self.store_snapshot(step["store_snapshot"], step.get("path", stores.PREFERENCES),
                                step.get("quiet", stores.SNAPSHOT_QUIET), step.get("within", stores.SNAPSHOT_WITHIN),
                                step.get("note"))

    # ---------- scenarios ----------
    def run(self, steps: list[dict]) -> None:
        d = self.driver
        for step in steps:
            note = step.get("note")
            if not any(key in step for key in NO_INPUT):
                self.require_unlocked(f"step {next((key for key in STEP_KEYS if key in step), '?')}")
            if any(key in step for key in READING_STEPS):
                self.reading_step(step)
                continue
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
                self.park("park step")
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
            elif "probe" in step:
                self.probe(step["probe"], step.get("send"), step.get("mods", ()), note, step.get("region"),
                           step.get("repeat", 1), step.get("quiet", PROBE_QUIET), step.get("timeout", PROBE_TIMEOUT),
                           step.get("stable_within", PROBE_STABLE), step.get("keep_pointer", False),
                           step.get("click_at"), step.get("release_at"))
            elif "resize" in step:
                d.resize(*step["resize"])
            elif "read_only" in step:
                self.read_only(step["read_only"], note)
