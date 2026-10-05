#!/usr/bin/env python3
"""Opening a large text patch in Compare on X11 (XWayland under GNOME), base against candidate, per open.

Usage, from the repository root with the QA virtual environment, the driver pinned off the app's CPUs:
  taskset -c 8,9 .local/qa-venv/bin/python3 compare_open_x11.py pilot MODE BINARY FIXTURE OUTDIR
  taskset -c 8,9 .local/qa-venv/bin/python3 compare_open_x11.py run MODE BASE CAND FIXTURE OUT.json PLAN
  .local/qa-venv/bin/python3 compare_open_x11.py analyze OUT.json
  .local/qa-venv/bin/python3 compare_open_x11.py export OUT.json RECORD.csv RECORD.json

The repository is found from this file's location (two levels up from docs/benchmarks/<record>/), or from
GITTURTLE_REPO. Runs go under GITTURTLE_EVIDENCE_RUNS (default /tmp/gitturtle-evidence/runs).
MODE is `compare` or `stash`. PLAN is a comma-separated list of `B` and `C` launches, fixed before the
first launch and kept in the output.

FIXTURE is make_compare_fixture.py's: History lists the stash first, then HEAD, whose commit changes
a-large.txt and b-large.txt (one 8,005-line patch each). Each launch goes through scripts/native_qa's
Session as `qa.py launch --input mutter` does (fresh HOME and XDG directories, QA Git identity, a
generated store with Midnight, Follow system off and 13 pt interface text, WAYLAND_DISPLAY and GIT_*
removed, DISPLAY=:0, GPUI_X11_SCALE_FACTOR=1, 1480x800, 5 s settle, activation, pointer parked) with
two differences: the argv is `taskset -c 0-7 BINARY FIXTURE`, and GITTURTLE_TRACE=1 with the app's
stderr read line by line on a pipe and stamped on receipt. Setup, not recorded: click HEAD's row
(History), click a-large.txt's row, which enters Compare with the file list focused, wait for its
patch, park the pointer. Then one key every PERIOD_NS through org.gnome.Mutter.RemoteDesktop
(NotifyKeyboardKeysym, never XTest): Down opens b-large.txt, Up opens a-large.txt, alternately. Each
key selects the other file, which clears the previous preview and editor, reads the content through
the main worker (its preview cache holds both after their first open) and builds the patch editor and
its decorations on the UI thread.

MODE `stash` opens the same files' stashed patches in the recovery inspector instead. Setup, not
recorded: click Changes, its Actions menu, "Browse stashes…", the stash, then a-large.txt, which
focuses the stash's file list; wait until the window is quiet; park the pointer. Down and Up then
alternate as above, and each selection clears the inspector's preview and editors, reads the content
through the inspector's own preview worker and builds its patch editor (and, in the candidate, keeps the
decorations handle the base drops). The stash inspector prints no trace, so its end point is the last
DamageNotify in the window (last_ms), and patch_ms, trace_ms and cpu_to_patch_ms are empty.

Phases in both modes: 1 cold open (b-large.txt's first, a worker cache miss),
6 warm-up opens, 40 recorded opens, 5 idle windows.

Per window, on CLOCK_MONOTONIC, from the stamp just before the key-down D-Bus call (t_send):
- raw_ms: the first XI2 RawKeyPress on the root window: Xwayland's dispatch of the key to X clients.
- trace_ms: the app's own `gitturtle.file_preview_frame_ms` value (select_file entry to the next-frame
  callback after the prepared preview was applied, before that frame's layout and paint);
  trace_rx_ms: when the driver read that stderr line.
- patch_ms: the first DamageNotify on the app's window read after trace_rx: the frame that draws the patch.
- first_ms: the first DamageNotify after the send, of any frame; last_ms: the last one in the window.
- frames: damage bursts in the window (events more than 1 ms apart count as separate frames).
- cpu_ms, wait_ms, slices: deltas of the app's main (UI) thread's /proc schedstat over the window;
  cpu_to_patch_ms: the UI thread's CPU from just before the send to the read just after the patch frame's
  DamageNotify; proc_cpu_ms: the sum over every thread of the process over the window.
- cpu, mhz: the CPU the UI thread last ran on, and its scaling_cur_freq, read just after the patch frame.
Percentiles are by nearest rank.
"""
import hashlib, json, os, platform, random, re, select, subprocess, sys, threading, time
from pathlib import Path

REPO = Path(os.environ.get("GITTURTLE_REPO") or Path(__file__).resolve().parents[3])
sys.path.insert(0, str(REPO / "scripts"))
from native_qa import identity, mutter, session, stores, x11  # noqa: E402

PIN = ["taskset", "-c", "0-7"]
DISPLAY = ":0"
SIZE = (1480, 800)
PERIOD_NS = 700_000_000
# Points as fractions of the 1480x800 window, checked in pilot frames on this host.
HEAD_ROW = (600 / 1480, 335 / 800)
FILE_A_ROW = (1300 / 1480, 515 / 800)
STASH_CLICKS = [(438 / 1480, 61 / 800, "Changes"), (1398 / 1480, 246 / 800, "Actions"),
                (1340 / 1480, 330 / 800, "Browse stashes"), (350 / 1480, 180 / 800, "the stash"),
                (700 / 1480, 213 / 800, "a-large.txt")]
COLD, WARMUP, RECORDED, IDLE = 1, 6, 40, 5
TRACE = re.compile(r"gitturtle\.file_preview_frame_ms=([0-9.]+)")
RUNS = Path(os.environ.get("GITTURTLE_EVIDENCE_RUNS", "/tmp/gitturtle-evidence/runs"))


def now():
    return time.monotonic_ns()


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
        with open(self.dirs.root / "app.stderr.log", "wb") as out:
            for raw in iter(self.proc.stderr.readline, b""):
                self.lines.append((now(), raw.decode(errors="replace").rstrip("\n")))
                out.write(raw)
                out.flush()

    def traces_after(self, t):
        return [(ts, float(m.group(1))) for ts, text in list(self.lines) if ts >= t and (m := TRACE.search(text))]


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
                    self.events.append((t, "damage", [ev.area.x, ev.area.y, ev.area.width, ev.area.height]))
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


def series(run, watch, pid, phase, keys, rows):
    """One key (Down, Up) or idle window (None) every PERIOD_NS; each window is closed by the next one's read."""
    start = now() + 20_000_000
    watch.pump(start)
    for i, key in enumerate(keys):
        t_pre = start + i * PERIOD_NS
        watch.pump(t_pre)
        st, pc = schedstat(pid), process_cpu(pid)
        if rows and rows[-1]["phase"] == phase and "cpu_ms" not in rows[-1]:
            close_row(rows[-1], st, pc)
        row = dict(phase=phase, i=i, key=key or "")
        rows.append(row)
        row["_st"], row["_pc"] = st, pc
        row["t_send"] = t_send = now()
        if key:
            sym = mutter.keysym(key)
            run.driver.remote.keysym(sym, True)
            row["dbus_ms"] = round((now() - t_send) / 1e6, 3)
            run.driver.remote.keysym(sym, False)

            def on_damage(t, row=row, st=st):
                if "cpu_to_patch_ms" in row or not run.traces_after(t_send):
                    return
                if run.traces_after(t_send)[0][0] <= t:
                    after = schedstat(pid)
                    row["cpu_to_patch_ms"] = round((after[0] - st[0]) / 1e6, 3)
                    row["cpu"], row["mhz"] = where(pid)

            watch.pump(t_send + PERIOD_NS - 30_000_000, on_damage)
    end = start + len(keys) * PERIOD_NS
    watch.pump(end)
    close_row(rows[-1], schedstat(pid), process_cpu(pid))


def close_row(row, after, pc):
    before = row.pop("_st")
    row["cpu_ms"] = round((after[0] - before[0]) / 1e6, 3)
    row["wait_ms"] = round((after[1] - before[1]) / 1e6, 3)
    row["slices"] = after[2] - before[2]
    row["proc_cpu_ms"] = round((pc - row.pop("_pc")) / 1e6, 3)


def attach(rows, events, traces):
    events = sorted(events)
    for k, row in enumerate(rows):
        lo = row["t_send"]
        hi = rows[k + 1]["t_send"] if k + 1 < len(rows) else lo + PERIOD_NS
        inside = [e for e in events if lo <= e[0] < hi]
        raw = [e for e in inside if e[1] == "raw"]
        dmg = [e[0] for e in inside if e[1] == "damage"]
        tr = [t for t in traces if lo <= t[0] < hi]
        row["raw_ms"] = round((raw[0][0] - lo) / 1e6, 3) if raw else None
        row["first_ms"] = round((dmg[0] - lo) / 1e6, 3) if dmg else None
        row["last_ms"] = round((dmg[-1] - lo) / 1e6, 3) if dmg else None
        row["traces"] = len(tr)
        row["trace_ms"] = tr[0][1] if tr else None
        row["trace_rx_ms"] = round((tr[0][0] - lo) / 1e6, 3) if tr else None
        patch = [t for t in dmg if tr and t >= tr[0][0]]
        row["patch_ms"] = round((patch[0] - lo) / 1e6, 3) if patch else None
        # The same frame found from the app's own value: the first damage at or after send + trace_ms.
        alt = [t for t in dmg if tr and t >= lo + tr[0][1] * 1e6]
        row["patch_alt_ms"] = round((alt[0] - lo) / 1e6, 3) if alt else None
        frames, last = 0, None
        for t in dmg:
            if last is None or t - last > 1_000_000:
                frames += 1
            last = t
        row["frames"] = frames
        row["damage_events"] = len(dmg)


def measure(mode, binary, fixture, run_dir, snapshot=False):
    width, height = SIZE
    prefs = stores.store_text("midnight", [], settings={"interface_text_size": 13})
    run = Pinned(Path(binary), Path(fixture), Path(run_dir), prefs, width=width, height=height, display=DISPLAY,
                 scale="1", backend="mutter", extra_env={"GITTURTLE_TRACE": "1"})
    out = dict(binary=os.path.basename(binary), run_dir=str(run_dir), started_utc=session.utc(),
               load1_before=os.getloadavg()[0])
    watch, rows = None, []
    try:
        run.launch()
        pid = run.proc.pid
        out["pid"] = pid
        out["exe_ok"] = os.readlink(f"/proc/{pid}/exe") == os.path.realpath(binary)
        out["affinity"] = sorted(os.sched_getaffinity(pid))
        assert out["exe_ok"] and out["affinity"] == list(range(8)), out
        out["window"] = list(run.driver.size())
        assert out["window"] == [width, height], out
        if snapshot:
            run.driver.snap().save(Path(run_dir) / "launch.png")
        run.require_unlocked("the setup clicks")
        if mode == "compare":
            run.driver.click(int(width * HEAD_ROW[0]), int(height * HEAD_ROW[1]), "HEAD's row")
            time.sleep(1.5)
            t_open = now()
            run.driver.click(int(width * FILE_A_ROW[0]), int(height * FILE_A_ROW[1]), "a-large.txt")
            deadline = time.time() + 10
            while not run.traces_after(t_open) and time.time() < deadline:
                time.sleep(0.05)
            if not run.traces_after(t_open):
                raise SystemExit("a-large.txt did not open in Compare")
            time.sleep(1.0)
        else:
            for fx, fy, what in STASH_CLICKS:
                run.driver.click(int(width * fx), int(height * fy), what)
                time.sleep(1.5)
            if run.driver.stable(timeout=10.0, quiet=1.0) is None:
                raise SystemExit("the stash preview did not settle")
        run.park("before keys")
        time.sleep(1.0)
        if snapshot:
            run.driver.snap().save(Path(run_dir) / "compare-a.png")
        watch = Watch(run.driver.w.id)
        keys = ["Down", "Up"]
        plan = (("cold", [keys[0]]), ("warmup", [keys[(1 + n) % 2] for n in range(WARMUP)]),
                ("recorded", [keys[(1 + WARMUP + n) % 2] for n in range(RECORDED)]), ("idle", [None] * IDLE))
        for phase, ks in plan:
            run.require_unlocked(f"the {phase} phase")
            run.driver.require_focus(f"the {phase} phase")
            series(run, watch, pid, phase, ks, rows)
            if snapshot and phase in ("cold", "warmup", "recorded"):
                run.driver.snap().save(Path(run_dir) / f"after-{phase}.png")
        attach(rows, watch.events, run.traces_after(0))
        out["raw_event_types"] = sorted({e[2] for e in watch.events if e[1] == "raw"})
        out["trace_lines"] = len(run.traces_after(0))
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
    return {m: pct(get(m), 50) for m in ("patch_ms", "last_ms", "trace_ms", "cpu_ms", "cpu_to_patch_ms",
                                         "frames")} | {"missing_patch": sum(r.get("patch_ms") is None for r in rec)}


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
        root = RUNS / f"decorations-{mode}-open-{int(time.time())}"
        root.mkdir(parents=True)
        meta = {"host": host(), "builds": {"base": build(base), "cand": build(cand)}, "fixture": fixture,
                "fixture_head": session.git(Path(fixture), "rev-parse", "HEAD"), "plan": plan, "mode": mode,
                "pinning": " ".join(PIN), "period_ms": PERIOD_NS / 1e6, "size": list(SIZE),
                "phases": {"cold": COLD, "warmup": WARMUP, "recorded": RECORDED, "idle": IDLE},
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
    "patch_ms": lambda r: r.get("patch_ms"),
    "app_patch_ms": lambda r: None if r.get("patch_ms") is None or r.get("raw_ms") is None
    else round(r["patch_ms"] - r["raw_ms"], 3),
    "trace_ms": lambda r: r.get("trace_ms"),
    "cpu_to_patch_ms": lambda r: r.get("cpu_to_patch_ms"),
    "cpu_ms": lambda r: r.get("cpu_ms"),
    "proc_cpu_ms": lambda r: r.get("proc_cpu_ms"),
    "last_ms_settle": lambda r: r.get("last_ms"),
    "app_last_ms": lambda r: None if r.get("last_ms") is None or r.get("raw_ms") is None
    else round(r["last_ms"] - r["raw_ms"], 3),
    "first_ms": lambda r: r.get("first_ms"),
    "raw_ms": lambda r: r.get("raw_ms"),
    "wait_ms": lambda r: r.get("wait_ms"),
}
DELTAS = ("patch_ms", "app_patch_ms", "trace_ms", "cpu_to_patch_ms", "cpu_ms", "proc_cpu_ms", "last_ms_settle",
          "app_last_ms")


def analyze(path):
    data = json.load(open(path))
    summary = {}
    by = {b: [l for l in data["launches"] if l["build"] == b] for b in ("base", "cand")}
    for phase in ("recorded", "cold"):
        for name, value in METRICS.items():
            groups = {b: [[value(r) for r in l["rows"] if r["phase"] == phase and value(r) is not None]
                          for l in ls] for b, ls in by.items()}
            cell = {}
            for b, g in groups.items():
                allv = [v for x in g for v in x]
                cell[b] = {"n": len(allv), "p50": pct(allv, 50), "p95": pct(allv, 95), "max": max(allv, default=None),
                           "mean": round(sum(allv) / len(allv), 3) if allv else None,
                           "launch_p50": [pct(x, 50) for x in g]}
            if name in DELTAS and phase == "recorded" and cell["base"]["n"] and cell["cand"]["n"]:
                for p in (50, 95):
                    cell[f"delta_p{p}"] = {"delta": round(cell["cand"][f"p{p}"] - cell["base"][f"p{p}"], 3),
                                           "ci95": boot(groups["base"], groups["cand"], lambda v, p=p: pct(v, p),
                                                        n=int(os.environ.get("BOOT_N", "4000")))}
            summary[f"{phase} {name}"] = cell
            print(phase, name, json.dumps(cell), flush=True)
    for b, ls in by.items():
        rows = [r for l in ls for r in l["rows"]]
        rec = [r for r in rows if r["phase"] == "recorded"]
        idle = [r for r in rows if r["phase"] == "idle"]
        summary[f"{b} counts"] = c = {
            "launches": len(ls), "recorded": len(rec), "missing_patch": sum(r.get("patch_ms") is None for r in rec),
            "patch_differs_from_alt": sum(r.get("patch_ms") != r.get("patch_alt_ms") for r in rec),
            "traces_not_one": sum(r.get("traces") != 1 for r in rec),
            "frames": {str(k): sum(r["frames"] == k for r in rec) for k in sorted({r["frames"] for r in rec})},
            "idle_frames": sum(r["frames"] for r in idle), "idle_cpu_p50": pct([r["cpu_ms"] for r in idle], 50),
            "mhz_p50": pct([r["mhz"] for r in rec if r.get("mhz")], 50),
            "mhz_p5": pct([r["mhz"] for r in rec if r.get("mhz")], 5),
            "mhz_max": max([r["mhz"] for r in rec if r.get("mhz")], default=None),
            "cpus": sorted({r["cpu"] for r in rec if r.get("cpu") is not None}),
            "load1_before": [l["load1_before"] for l in ls], "exits": [l["exit"] for l in ls],
            "fixture_unchanged": all(l["fixture_unchanged"] for l in ls)}
        print(b, json.dumps(c), flush=True)
    data["summary"] = summary
    json.dump(data, open(path, "w"))


def export(path, csv_out, json_out):
    """The raw rows as CSV, and everything else (host, builds, plan, per-launch metadata, summary) as JSON."""
    data = json.load(open(path))
    cols = ["launch", "build", "phase", "i", "key", "t_ms", "dbus_ms", "raw_ms", "trace_ms", "trace_rx_ms",
            "first_ms", "patch_ms", "patch_alt_ms", "last_ms", "frames", "damage_events", "traces", "cpu_ms",
            "cpu_to_patch_ms", "proc_cpu_ms", "wait_ms", "slices", "cpu", "mhz"]
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
