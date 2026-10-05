#!/usr/bin/env python3
"""Theme application in the running app on X11 (XWayland under GNOME), base against candidate, per switch.

Usage, from the repository root with the QA virtual environment, the driver pinned off the app's CPUs:
  taskset -c 8,9 .local/qa-venv/bin/python3 theme_switch_x11.py pilot MODE BINARY FIXTURE OUTDIR
  taskset -c 8,9 .local/qa-venv/bin/python3 theme_switch_x11.py run MODE BASE CAND FIXTURE OUT.json PLAN
  .local/qa-venv/bin/python3 theme_switch_x11.py analyze OUT.json
  .local/qa-venv/bin/python3 theme_switch_x11.py export OUT.json RECORD.csv RECORD.json

The repository is found from GITTURTLE_REPO, or from this file's location (two levels up from
docs/benchmarks/<record>/). Runs go under GITTURTLE_EVIDENCE_RUNS (default /tmp/gitturtle-evidence/runs).
MODE is `builtin` or `omarchy`. PLAN is a comma-separated list of `B` and `C` launches, fixed before the
first launch and kept in the output. FIXTURE is make_theme_fixture.py's: the themes budget fixture.

Each launch goes through scripts/native_qa's Session as `qa.py launch --input mutter` does (fresh HOME and
XDG directories, QA Git identity, a generated store with Follow system off and 13 pt interface text,
WAYLAND_DISPLAY and GIT_* removed, DISPLAY=:0, GPUI_X11_SCALE_FACTOR=1, 1480x1100, 5 s settle,
activation, pointer parked) with two differences: the argv is `taskset -c 0-7 BINARY FIXTURE`, and
GITTURTLE_TRACE=1 with the app's stderr read line by line on a pipe and stamped on receipt.

Setup, not recorded, the spec's budget state: History's Older control (1-1000, waits for its
history_page_frame_ms line), HEAD's row (row 2, below the experiment branch's newer commit),
large-module.ts in its changed files (Compare; waits for file_preview_frame_ms), Split.

MODE builtin: store theme Midnight, no Omarchy state. Settings opens through its toolbar button with
the Split comparison retained behind it, and every one of the twenty built-in cards is on screen
unscrolled at 1480x1100; the pointer is parked off the window. Each switch is a real click through
org.gnome.Mutter.RemoteDesktop (never XTest): one relative motion onto the card's preview at t0, the
button pressed at t0 + 150 ms and released at t0 + 250 ms (t_send: the click, and choose_theme, fire on
release), and at t_send + 200 ms one motion off the window's right edge, before the 500 ms tooltip
delay that started at t0 can show a tooltip. The cards are clicked in ThemeChoice::ALL order from
Midnight: 4 warm-up switches (Graphite to Nord), then 80 recorded (Porcelain onward, four full cycles,
so each palette is the target of 4 recorded switches per launch). The store is read at the end of each
window and must name the expected palette.

MODE omarchy: store theme `omarchy`; HOME is seeded with .local/state/omarchy/current/theme.name and
current/theme/colors.toml (the app's tests/fixtures/omarchy/tokyo-night.toml). The app stays on the
Split comparison. Each switch performs omarchy-theme-set's writes, in its order, in current/: stage
next-theme/ with the other theme's colors.toml, remove theme/, rename next-theme/ to theme/, then write
theme.name. The themes alternate Catppuccin Latte and Tokyo Night: 4 warm-up switches, 40 recorded.

Both modes end with 5 idle windows. Per window, on CLOCK_MONOTONIC, from the stamp just before the Space
key-down D-Bus call or the first write (t_send):
- cpu_150_ms (builtin): the UI thread's CPU from just before the release to t_send + 150 ms (the window
  of the 2026-09-23 picker record, which ends before the pointer leaves).
- trace_ms: the app's own gitturtle.theme_apply_frame_ms (choose_theme entry to the next-frame callback)
  or gitturtle.omarchy_apply_frame_ms (the reread's arrival on the UI thread to the next-frame callback);
  trace_rx_ms: when the driver read that line; reread_ms: gitturtle.omarchy_reread_ms (omarchy).
- frame_ms: the first DamageNotify on the app's window read after trace_rx: the frame drawn in the new theme.
- first_ms / last_ms: the first and last DamageNotify in the window; frames: damage bursts (> 1 ms apart).
- cpu_to_frame_ms: the app's UI thread's CPU (/proc schedstat) from just before t_send to the read just
  after the frame's DamageNotify; cpu_ms, wait_ms, slices: the same thread over the whole window;
  proc_cpu_ms: every thread of the process over the window; cpu, mhz: the CPU the UI thread last ran on
  and its scaling_cur_freq, read just after the frame.
Percentiles are by nearest rank.
"""
import hashlib, json, os, platform, random, re, select, shutil, subprocess, sys, threading, time
from pathlib import Path

HERE = Path(__file__).resolve()
REPO = Path(os.environ.get("GITTURTLE_REPO") or HERE.parents[3])
sys.path.insert(0, str(REPO / "scripts"))
from native_qa import identity, mutter, session, stores, x11  # noqa: E402

PIN = ["taskset", "-c", "0-7"]
DISPLAY = ":0"
SIZE = (1480, 1100)
PRESS_NS, RELEASE_NS, CPU_WINDOW_NS, LEAVE_NS = 150_000_000, 250_000_000, 150_000_000, 200_000_000
PERIOD_NS = {"builtin": 1_200_000_000, "omarchy": 1_500_000_000}
# Points in the 1480-wide window, checked in pilot frames on this host (top-anchored layout).
OLDER = (1124, 156)
HEAD_ROW = (600, 300)
FILE_ROW = (1300, 642)
SPLIT = (1014, 156)
SETTINGS = (1455, 62)
OFF_WINDOW = (SIZE[0] + 80, 500)


def card_point(name):
    """The centre of a card's preview: four columns, the light group's two rows, then the dark group's three."""
    k = CARDS.index(name)
    row, col = divmod(k, 4)
    return 267 + 190 * col, (330, 468, 640, 778, 916)[row]


# The picker's cards in grid order: the light group, then the dark group, as the store names them.
CARDS = ["daylight", "porcelain", "sandstone", "solarized_light", "one_light", "rose_pine_dawn", "alucard",
         "kanagawa_lotus", "midnight", "graphite", "tokyo_night", "catppuccin_mocha", "nord", "deep_sea",
         "ember", "solarized_dark", "one_dark", "rose_pine", "dracula", "kanagawa_wave"]
OMARCHY = {"tokyo-night": "tokyo-night.toml", "catppuccin-latte": "catppuccin-latte.toml"}
WARMUP = {"builtin": 4, "omarchy": 4}
RECORDED = {"builtin": 80, "omarchy": 40}
ALL_ORDER = ["midnight", "daylight", "graphite", "tokyo_night", "catppuccin_mocha", "nord", "porcelain", "sandstone",
             "deep_sea", "ember", "solarized_dark", "solarized_light", "one_dark", "one_light", "rose_pine",
             "rose_pine_dawn", "dracula", "alucard", "kanagawa_wave", "kanagawa_lotus"]
IDLE = 5
TRACES = {"builtin": re.compile(r"gitturtle\.theme_apply_frame_ms=([0-9.]+)"),
          "omarchy": re.compile(r"gitturtle\.omarchy_apply_frame_ms=([0-9.]+)")}
REREAD = re.compile(r"gitturtle\.omarchy_reread_ms=([0-9.]+)")
ANY_TRACE = re.compile(r"gitturtle\.([a-z_]+)=([0-9.]+)")
RUNS = Path(os.environ.get("GITTURTLE_EVIDENCE_RUNS", "/tmp/gitturtle-evidence/runs"))
OMARCHY_ROOT = ".local/state/omarchy/current"


def now():
    return time.monotonic_ns()


def omarchy_bytes(name):
    return (REPO / "crates/app/tests/fixtures/omarchy" / OMARCHY[name]).read_bytes()


class Pinned(session.Session):
    """Session.launch with the app under `taskset -c 0-7` and its stderr on a stamped pipe."""

    def launch(self):
        self.require_unlocked("the launch")
        argv = PIN + [str(self.binary), str(self.fixture)]
        self.applog = open(self.dirs.root / "app.log", "wb")
        self.proc = subprocess.Popen(argv, env=self.env, stdout=self.applog, stderr=subprocess.PIPE)
        self.lines = []  # (t_ns, text)
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        self.log["launch"] = dict(argv=argv, pid=self.proc.pid, at_utc=session.utc(), input=self.input)
        dsp = x11.connect(self.env["DISPLAY"])
        window = x11.find_window(dsp, self.proc.pid)
        self.remote = mutter.RemoteDesktop.connect()
        self.driver = mutter.MutterDriver(dsp, window, self.log["input"], self.remote)
        self.size_window()
        time.sleep(self.settle)
        self.driver.activate()
        self.park("launch")

    def _read(self):
        with open(self.dirs.root / "app.stderr.txt", "wb") as out:
            for raw in iter(self.proc.stderr.readline, b""):
                self.lines.append((now(), raw.decode(errors="replace").rstrip("\n")))
                out.write(raw)
                out.flush()

    def matches(self, pattern, t=0):
        return [(ts, float(m.group(1))) for ts, text in list(self.lines) if ts >= t and (m := pattern.search(text))]

    def wait_line(self, pattern, t, what, timeout=10.0):
        deadline = time.time() + timeout
        while not self.matches(pattern, t) and time.time() < deadline:
            time.sleep(0.05)
        if not self.matches(pattern, t):
            raise SystemExit(f"no {what} line")


class Watch:
    """A second X connection: DamageNotify on the app window and XI2 raw key presses on the root, stamped on receipt."""

    def __init__(self, wid):
        from Xlib import display as xdisplay
        from Xlib.ext import damage, xinput

        self.d = xdisplay.Display(DISPLAY)
        self.d.set_error_handler(lambda *a: None)
        self.d.damage_query_version()
        self.damage_event = self.d.query_extension("DAMAGE").first_event + damage.DamageNotifyCode
        self.xi_opcode = self.d.query_extension("XInputExtension").major_opcode
        self.d.xinput_query_version()
        self.win = self.d.create_resource_object("window", wid)
        self.win.damage_create(damage.DamageReportRawRectangles)
        self.d.screen().root.xinput_select_events([(xinput.AllDevices, xinput.RawKeyPressMask)])
        self.d.sync()
        self.events = []  # (t_ns, kind, detail)

    def pump(self, deadline, on_damage=None):
        fd = self.d.fileno()
        while True:
            while self.d.pending_events():
                ev = self.d.next_event()
                t = now()
                if ev.type == self.damage_event:
                    self.events.append((t, "damage", None))
                    if on_damage:
                        on_damage(t)
                elif ev.type == 35 and getattr(ev, "extension", None) == self.xi_opcode:
                    self.events.append((t, "raw", int(ev.evtype)))
            left = deadline - now()
            if left <= 0:
                return
            select.select([fd], [], [], left / 1e9)
            self.d.pending_events()

    def close(self):
        try:
            self.d.close()
        except Exception:
            pass


def schedstat(pid, tid=None):
    exec_ns, wait_ns, slices = open(f"/proc/{pid}/task/{tid or pid}/schedstat").read().split()
    return int(exec_ns), int(wait_ns), int(slices)


def process_cpu(pid):
    total = 0
    for tid in os.listdir(f"/proc/{pid}/task"):
        try:
            total += schedstat(pid, tid)[0]
        except OSError:
            pass
    return total


def where(pid):
    stat = open(f"/proc/{pid}/task/{pid}/stat").read()
    cpu = int(stat[stat.rfind(")") + 2:].split()[36])
    try:
        khz = int(open(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq").read())
    except OSError:
        khz = None
    return cpu, None if khz is None else round(khz / 1000)


def omarchy_switch(root, name):
    """omarchy-theme-set's writes, in its order: stage next-theme/, remove theme/, move it into place, theme.name."""
    nxt = root / "next-theme"
    (nxt / "backgrounds").mkdir(parents=True)
    (nxt / "colors.toml").write_bytes(omarchy_bytes(name))
    shutil.rmtree(root / "theme")
    os.rename(nxt, root / "theme")
    (root / "theme.name").write_text(name + "\n")


def store_theme(run):
    try:
        theme = json.loads(run.dirs.preferences.read_bytes())["settings"]["theme"]
    except (OSError, ValueError, KeyError):
        return None
    return theme if isinstance(theme, str) else json.dumps(theme)


def series(mode, run, watch, pid, phase, steps, rows):
    """One switch (or idle window, None) every PERIOD_NS. A builtin step is (direction, expected palette);
    an omarchy step is the theme name to switch to."""
    period = PERIOD_NS[mode]
    trace = TRACES[mode]
    start = now() + 20_000_000
    watch.pump(start)
    for i, step in enumerate(steps):
        t0 = start + i * period
        watch.pump(t0)
        if mode == "builtin" and step:
            run.driver.move(*card_point(step), step)
            watch.pump(t0 + PRESS_NS)
            run.driver.remote.button(mutter.BUTTONS[1], True)
            watch.pump(t0 + RELEASE_NS)
        st, pc = schedstat(pid), process_cpu(pid)
        if rows and rows[-1]["phase"] == phase and "cpu_ms" not in rows[-1]:
            close_row(run, rows[-1], st, pc)
        row = dict(phase=phase, i=i)
        rows.append(row)
        row["_st"], row["_pc"] = st, pc
        row["t_send"] = t_send = now()
        if step:
            if mode == "builtin":
                row["expected"] = step
                run.driver.remote.button(mutter.BUTTONS[1], False)
                row["dbus_ms"] = round((now() - t_send) / 1e6, 3)
            else:
                row["expected"] = step
                omarchy_switch(run.dirs.paths["HOME"] / OMARCHY_ROOT, step)
                row["write_ms"] = round((now() - t_send) / 1e6, 3)

            def on_damage(t, row=row, st=st, t_send=t_send):
                if "cpu_to_frame_ms" in row:
                    return
                got = run.matches(trace, t_send)
                if got and got[0][0] <= t:
                    after = schedstat(pid)
                    row["cpu_to_frame_ms"] = round((after[0] - st[0]) / 1e6, 3)
                    row["cpu"], row["mhz"] = where(pid)

            if mode == "builtin":
                watch.pump(t_send + CPU_WINDOW_NS, on_damage)
                row["cpu_150_ms"] = round((schedstat(pid)[0] - st[0]) / 1e6, 3)
                watch.pump(t_send + LEAVE_NS, on_damage)
                run.driver.move(*OFF_WINDOW, "off the window")
            watch.pump(t0 + period - 40_000_000, on_damage)
        else:
            row["expected"] = ""
    end = start + len(steps) * period
    watch.pump(end)
    close_row(run, rows[-1], schedstat(pid), process_cpu(pid))


def close_row(run, row, after, pc):
    before = row.pop("_st")
    row["cpu_ms"] = round((after[0] - before[0]) / 1e6, 3)
    row["wait_ms"] = round((after[1] - before[1]) / 1e6, 3)
    row["slices"] = after[2] - before[2]
    row["proc_cpu_ms"] = round((pc - row.pop("_pc")) / 1e6, 3)
    row["store_theme"] = store_theme(run)


def attach(mode, rows, events, lines):
    events = sorted(events)
    trace = TRACES[mode]
    for k, row in enumerate(rows):
        lo = row["t_send"]
        hi = rows[k + 1]["t_send"] if k + 1 < len(rows) else lo + PERIOD_NS[mode]
        inside = [e for e in events if lo <= e[0] < hi]
        raw = [e for e in inside if e[1] == "raw"]
        dmg = [e[0] for e in inside if e[1] == "damage"]
        tr = [(t, float(m.group(1))) for t, text in lines if lo <= t < hi and (m := trace.search(text))]
        rr = [(t, float(m.group(1))) for t, text in lines if lo <= t < hi and (m := REREAD.search(text))]
        other = sorted({m.group(1) for t, text in lines if lo <= t < hi and (m := ANY_TRACE.search(text))}
                       - {"theme_apply_frame_ms", "omarchy_apply_frame_ms", "omarchy_reread_ms"})
        row["first_ms"] = round((dmg[0] - lo) / 1e6, 3) if dmg else None
        row["last_ms"] = round((dmg[-1] - lo) / 1e6, 3) if dmg else None
        row["traces"] = len(tr)
        row["trace_ms"] = tr[0][1] if tr else None
        row["trace_rx_ms"] = round((tr[0][0] - lo) / 1e6, 3) if tr else None
        row["rereads"] = len(rr)
        row["reread_ms"] = rr[0][1] if rr else None
        row["other_traces"] = ";".join(other)
        frame = [t for t in dmg if tr and t >= tr[0][0]]
        row["frame_ms"] = round((frame[0] - lo) / 1e6, 3) if frame else None
        frames, last = 0, None
        for t in dmg:
            if last is None or t - last > 1_000_000:
                frames += 1
            last = t
        row["frames"] = frames
        row["damage_events"] = len(dmg)


def plan_steps(mode):
    if mode == "builtin":
        steps = [ALL_ORDER[(2 + n) % 20] for n in range(WARMUP[mode] + RECORDED[mode])]
        return [("warmup", steps[:WARMUP[mode]]), ("recorded", steps[WARMUP[mode]:]), ("idle", [None] * IDLE)]
    names = ["catppuccin-latte", "tokyo-night"]
    seq = [names[n % 2] for n in range(WARMUP[mode] + RECORDED[mode])]
    return [("warmup", seq[:WARMUP[mode]]), ("recorded", seq[WARMUP[mode]:]), ("idle", [None] * IDLE)]


def measure(mode, binary, fixture, run_dir, snapshot=False):
    width, height = SIZE
    if mode == "builtin":
        prefs = stores.store_text("midnight", [], settings={"interface_text_size": 13})
        home_files = {}
    else:
        prefs = stores.store_text("omarchy", [], settings={"interface_text_size": 13})
        home_files = {f"{OMARCHY_ROOT}/theme.name": b"tokyo-night\n",
                      f"{OMARCHY_ROOT}/theme/colors.toml": omarchy_bytes("tokyo-night")}
    run = Pinned(Path(binary), Path(fixture), Path(run_dir), prefs, width=width, height=height, display=DISPLAY,
                 scale="1", backend="mutter", extra_env={"GITTURTLE_TRACE": "1"}, home_files=home_files)
    out = dict(binary=os.path.basename(binary), run_dir=str(run_dir), started_utc=session.utc(),
               load1_before=os.getloadavg()[0], store_sha256=identity.sha256_bytes(prefs),
               home_files={k: identity.sha256_bytes(v) for k, v in home_files.items()})
    watch, rows = None, []
    snap = (lambda name: run.driver.snap().save(Path(run_dir) / f"{name}.png")) if snapshot else (lambda name: None)
    try:
        run.launch()
        pid = run.proc.pid
        out["pid"] = pid
        out["exe_ok"] = os.readlink(f"/proc/{pid}/exe") == os.path.realpath(binary)
        out["affinity"] = sorted(os.sched_getaffinity(pid))
        assert out["exe_ok"] and out["affinity"] == list(range(8)), out
        out["window"] = list(run.driver.size())
        assert out["window"] == [width, height], out
        snap("launch")
        run.require_unlocked("the setup clicks")
        t = now()
        run.driver.click(*OLDER, "Older")
        run.wait_line(re.compile(r"gitturtle\.history_page_frame_ms=([0-9.]+)"), t, "history page")
        time.sleep(1.0)
        run.driver.click(*HEAD_ROW, "HEAD's row")
        time.sleep(1.5)
        t = now()
        run.driver.click(*FILE_ROW, "large-module.ts")
        run.wait_line(re.compile(r"gitturtle\.file_preview_frame_ms=([0-9.]+)"), t, "file preview")
        time.sleep(1.0)
        run.driver.click(*SPLIT, "Split")
        time.sleep(1.5)
        snap("split")
        if mode == "builtin":
            run.driver.click(*SETTINGS, "Settings")
            time.sleep(1.5)
        run.park("before switches")
        time.sleep(1.5)
        snap("before")
        watch = Watch(run.driver.w.id)
        for phase, steps in plan_steps(mode):
            run.require_unlocked(f"the {phase} phase")
            run.driver.require_focus(f"the {phase} phase")
            series(mode, run, watch, pid, phase, steps, rows)
            snap(f"after-{phase}")
        attach(mode, rows, watch.events, list(run.lines))
        out["trace_lines"] = len(run.matches(TRACES[mode]))
    finally:
        if watch is not None:
            watch.close()
        out["exit"] = run.close()
        out["fixture_unchanged"] = run.log.get("fixture_unchanged")
        out["ended_utc"] = session.utc()
        out["load1_after"] = os.getloadavg()[0]
    t0 = rows[0]["t_send"] if rows else 0
    for row in rows:
        row["t_ms"] = round((row.pop("t_send") - t0) / 1e6, 3)
    out["rows"] = rows
    out["mismatched_store"] = sum(1 for r in rows if r["expected"] and r.get("store_theme") != (
        r["expected"] if mode == "builtin" else "omarchy"))
    return out


def pct(values, p):
    v = sorted(values)
    return v[max(0, -(-len(v) * p // 100) - 1)] if v else None


def host():
    cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None)
    read = lambda p: open(p).read().strip() if os.path.exists(p) else None
    return {"kernel": platform.release(), "cpu": cpu, "logical_cpus": os.cpu_count(),
            "governor": read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
            "epp": read("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference"),
            "no_turbo": read("/sys/devices/system/cpu/intel_pstate/no_turbo"),
            "driver_affinity": sorted(os.sched_getaffinity(0)), "display": DISPLAY,
            "session": os.environ.get("XDG_SESSION_TYPE"), "desktop": os.environ.get("XDG_CURRENT_DESKTOP")}


def build(b):
    d = identity.describe(Path(b))
    return {"binary": os.path.basename(b), "sha256": d.get("sha256"), "build_info": d.get("build_info")}


def summary_line(res):
    rec = [r for r in res["rows"] if r["phase"] == "recorded"]
    get = lambda m: [r[m] for r in rec if r.get(m) is not None]
    return {m: pct(get(m), 50) for m in ("trace_ms", "frame_ms", "cpu_to_frame_ms", "cpu_ms", "frames")} | {
        "missing_frame": sum(r.get("frame_ms") is None for r in rec), "traces_not_one": sum(r["traces"] != 1 for r in rec),
        "mismatched_store": res.get("mismatched_store")}


def main():
    cmd = sys.argv[1]
    if cmd == "pilot":
        mode, binary, fixture, outdir = sys.argv[2:6]
        res = measure(mode, os.path.abspath(binary), os.path.abspath(fixture), os.path.abspath(outdir), snapshot=True)
        json.dump(res, open(Path(outdir) / "pilot.json", "w"), indent=1)
        print(json.dumps({k: res[k] for k in res if k != "rows"}))
        print(json.dumps(summary_line(res)))
        return
    if cmd == "run":
        mode, base, cand, fixture, out, plan = sys.argv[2:8]
        base, cand, fixture = (os.path.abspath(p) for p in (base, cand, fixture))
        launches = plan.split(",")
        root = RUNS / f"theme-switch-{mode}-{int(time.time())}"
        root.mkdir(parents=True)
        meta = {"host": host(), "builds": {"base": build(base), "cand": build(cand)}, "fixture": fixture,
                "fixture_head": session.git(Path(fixture), "rev-parse", "HEAD"), "plan": plan, "mode": mode,
                "pinning": " ".join(PIN), "period_ms": PERIOD_NS[mode] / 1e6, "click_ms": {"press": PRESS_NS / 1e6, "release": RELEASE_NS / 1e6, "cpu_window": CPU_WINDOW_NS / 1e6, "leave": LEAVE_NS / 1e6},
                "size": list(SIZE), "phases": {"warmup": WARMUP[mode], "recorded": RECORDED[mode], "idle": IDLE},
                "percentile": "nearest rank", "started_utc": session.utc(), "load_before": os.getloadavg(),
                "driver_sha256": hashlib.sha256(open(__file__, "rb").read()).hexdigest()}
        results = []
        for n, who in enumerate(launches):
            binary = base if who == "B" else cand
            res = measure(mode, binary, fixture, root / f"{n:02d}-{who}")
            res.update(n=n, build="base" if who == "B" else "cand")
            results.append(res)
            print(json.dumps({"n": n, "build": res["build"], **summary_line(res), "load1": res["load1_before"],
                              "exit": res["exit"], "fixture_unchanged": res["fixture_unchanged"]}), flush=True)
            json.dump({**meta, "launches": results}, open(out, "w"))
            time.sleep(2.0)
        meta.update(ended_utc=session.utc(), load_after=os.getloadavg())
        json.dump({**meta, "launches": results}, open(out, "w"))
        return
    if cmd == "analyze":
        analyze(sys.argv[2])
        return
    if cmd == "export":
        export(*sys.argv[2:5])
        return
    raise SystemExit(__doc__)


def boot(groups_a, groups_b, stat, n=4000, seed=1):
    """Two-level bootstrap of stat(candidate) - stat(base): launches with replacement, then samples within each."""
    rng = random.Random(seed)
    out = []
    for _ in range(n):
        def draw(groups):
            vals = []
            for g in (rng.choice(groups) for _ in groups):
                vals.extend(rng.choice(g) for _ in g)
            return stat(vals)
        out.append(draw(groups_b) - draw(groups_a))
    out.sort()
    return round(out[int(0.025 * n)], 3), round(out[int(0.975 * n) - 1], 3)


METRICS = {
    "trace_ms": lambda r: r.get("trace_ms"),
    "frame_ms": lambda r: r.get("frame_ms"),
    "cpu_to_frame_ms": lambda r: r.get("cpu_to_frame_ms"),
    "cpu_ms": lambda r: r.get("cpu_ms"),
    "proc_cpu_ms": lambda r: r.get("proc_cpu_ms"),
    "reread_ms": lambda r: r.get("reread_ms"),
    "cpu_150_ms": lambda r: r.get("cpu_150_ms"),
    "wait_ms": lambda r: r.get("wait_ms"),
}
DELTAS = ("trace_ms", "frame_ms", "cpu_to_frame_ms", "cpu_150_ms", "cpu_ms", "proc_cpu_ms", "reread_ms")


def cell_of(groups, name, boot_n):
    cell = {}
    for b, g in groups.items():
        allv = [v for x in g for v in x]
        cell[b] = {"n": len(allv), "p50": pct(allv, 50), "p95": pct(allv, 95), "max": max(allv, default=None),
                   "mean": round(sum(allv) / len(allv), 3) if allv else None, "launch_p50": [pct(x, 50) for x in g]}
    if name in DELTAS and boot_n and cell["base"]["n"] and cell["cand"]["n"]:
        for p in (50, 95):
            cell[f"delta_p{p}"] = {"delta": round(cell["cand"][f"p{p}"] - cell["base"][f"p{p}"], 3),
                                   "ci95": boot(groups["base"], groups["cand"], lambda v, p=p: pct(v, p), n=boot_n)}
    return cell


def analyze(path):
    data = json.load(open(path))
    summary = {}
    boot_n = int(os.environ.get("BOOT_N", "4000"))
    by = {b: [l for l in data["launches"] if l["build"] == b] for b in ("base", "cand")}
    for phase in ("recorded", "idle"):
        for name, value in METRICS.items():
            groups = {b: [[value(r) for r in l["rows"] if r["phase"] == phase and value(r) is not None]
                          for l in ls] for b, ls in by.items()}
            if not any(any(g) for g in groups.values()):
                continue
            summary[f"{phase} {name}"] = cell = cell_of(groups, name, boot_n if phase == "recorded" else 0)
            print(phase, name, json.dumps(cell), flush=True)
    # Per palette (builtin) or per target theme (omarchy): pooled p50/p95/max and the p50 delta.
    targets = sorted({r["expected"] for l in data["launches"] for r in l["rows"] if r["phase"] == "recorded"})
    for target in targets:
        for name in ("cpu_to_frame_ms", "cpu_150_ms", "trace_ms", "frame_ms"):
            groups = {b: [[METRICS[name](r) for r in l["rows"] if r["phase"] == "recorded" and r["expected"] == target
                           and METRICS[name](r) is not None] for l in ls] for b, ls in by.items()}
            if not all(any(g) for g in groups.values()):
                continue
            cell = cell_of(groups, name, 0)
            cell["delta_p50"] = round(cell["cand"]["p50"] - cell["base"]["p50"], 3)
            summary[f"target {target} {name}"] = cell
    for b, ls in by.items():
        rows = [r for l in ls for r in l["rows"]]
        rec = [r for r in rows if r["phase"] == "recorded"]
        idle = [r for r in rows if r["phase"] == "idle"]
        summary[f"{b} counts"] = c = {
            "launches": len(ls), "recorded": len(rec), "missing_frame": sum(r.get("frame_ms") is None for r in rec),
            "traces_not_one": sum(r.get("traces") != 1 for r in rec),
            "rereads_not_one": sum(r.get("rereads") != 1 for r in rec) if data["mode"] == "omarchy" else None,
            "other_traces": sorted({r["other_traces"] for r in rows if r.get("other_traces")}),
            "mismatched_store": sum(l.get("mismatched_store", 0) for l in ls),
            "frames": {str(k): sum(r["frames"] == k for r in rec) for k in sorted({r["frames"] for r in rec})},
            "idle_frames": sum(r["frames"] for r in idle), "idle_cpu_p50": pct([r["cpu_ms"] for r in idle], 50),
            "trace_over_period": None,
            "mhz_p50": pct([r["mhz"] for r in rec if r.get("mhz")], 50),
            "mhz_p5": pct([r["mhz"] for r in rec if r.get("mhz")], 5),
            "cpus": sorted({r["cpu"] for r in rec if r.get("cpu") is not None}),
            "load1_before": [l["load1_before"] for l in ls], "exits": [l["exit"] for l in ls],
            "fixture_unchanged": all(l["fixture_unchanged"] for l in ls)}
        print(b, json.dumps(c), flush=True)
    data["summary"] = summary
    json.dump(data, open(path, "w"))


def export(path, csv_out, json_out):
    """The raw rows as CSV, and everything else (host, builds, plan, per-launch metadata, summary) as JSON."""
    data = json.load(open(path))
    cols = ["launch", "build", "phase", "i", "expected", "store_theme", "t_ms", "dbus_ms", "write_ms",
            "trace_ms", "trace_rx_ms", "frame_ms", "first_ms", "last_ms", "frames", "damage_events", "traces",
            "rereads", "reread_ms", "other_traces", "cpu_to_frame_ms", "cpu_150_ms", "cpu_ms", "proc_cpu_ms", "wait_ms", "slices",
            "cpu", "mhz"]
    with open(csv_out, "w") as f:
        f.write(",".join(cols) + "\n")
        for l in data["launches"]:
            for r in l["rows"]:
                vals = [l["n"], l["build"]] + [r.get(c) for c in cols[2:]]
                f.write(",".join("" if v is None else str(v) for v in vals) + "\n")
    meta = {k: v for k, v in data.items() if k != "launches"}
    meta["launches"] = [{k: v for k, v in l.items() if k != "rows"} for l in data["launches"]]
    for l in meta["launches"]:
        l["run_dir"] = os.path.basename(l["run_dir"])
    meta["fixture"] = os.path.basename(meta["fixture"])
    json.dump(meta, open(json_out, "w"), indent=1)


if __name__ == "__main__":
    main()
