#!/usr/bin/env python3
"""History wheel-scroll cost on X11 (XWayland under GNOME), base against candidate, per notch.

Usage, from the repository root with the QA virtual environment, the driver pinned off the app's CPUs:
  taskset -c 8,9 .local/qa-venv/bin/python3 history_scroll_x11.py pilot BINARY FIXTURE WxH OUTDIR
  taskset -c 8,9 .local/qa-venv/bin/python3 history_scroll_x11.py run BASE CAND FIXTURE OUT.json PLAN
  .local/qa-venv/bin/python3 history_scroll_x11.py analyze OUT.json

PLAN is a comma-separated list of launches, each `B` or `C` and a size, such as
`B1480x800,C1480x800,C461x490,B461x490`; it is fixed before the first launch and kept in the output.

Each launch goes through scripts/native_qa's Session, as `qa.py launch --input mutter` does: a fresh
HOME and XDG directories under the run directory, the QA Git identity, a generated store (Midnight,
Follow system off, interface text 13 pt), WAYLAND_DISPLAY and GIT_* removed, DISPLAY=:0,
GPUI_X11_SCALE_FACTOR=1, the window found by _NET_WM_PID, its WM_NORMAL_HINTS minimum lowered to
400x420 for a size below the app's 1000x680, resized, 5 s settle, activated and the pointer parked.
The only difference from Session.launch is the argv, `taskset -c 0-7 BINARY FIXTURE`; the
driver checks the process's affinity and /proc/PID/exe. Input goes through
org.gnome.Mutter.RemoteDesktop (NotifyPointerAxisDiscrete, one discrete step per notch), never
XTest, with the pointer glided onto the History list and confirmed there by XWayland.

Per launch: 20 warm-up notches (10 down, 10 up), 20 idle windows, 120 recorded notches (30 down,
30 up, twice), 20 idle windows. One notch or idle window every 150 ms. The lock state is checked
and the pointer confirmed on the list before each phase.

Per 150 ms window (from just before one send to just before the next), on CLOCK_MONOTONIC:
- send: the driver's stamp just before the synchronous D-Bus call; dbus_ms is its return.
- raw_ms: receipt of the first XI2 raw event (RawButtonPress, RawMotion) on the root window after
  the send: Xwayland's dispatch of the notch to X clients. None when Xwayland reports none.
- damage_ms: receipt of the first X DamageNotify (raw rectangles) on the app's window after the
  send: a frame the app presented, copied or flipped into the window by Xwayland.
- frames: damage bursts in the window (events more than 1 ms apart count as separate frames).
- cpu_ms, wait_ms, slices: deltas of /proc/PID/task/PID/schedstat of the app's main (UI) thread
  over the window: CPU time, time runnable but waiting, and timeslices.
- cpu, mhz: the CPU the main thread last ran on at the end of the window, and its scaling_cur_freq.
- cpu_busy, mhz_busy: the same, read just after the notch's first DamageNotify, while the core was busy.
Percentiles are by nearest rank.
"""
import hashlib, json, os, platform, random, select, signal, subprocess, sys, time
from pathlib import Path

REPO = Path(os.environ.get("GITTURTLE_REPO") or Path(__file__).resolve().parents[3])
sys.path.insert(0, str(REPO / "scripts"))
from native_qa import identity, runenv, session, stores, x11  # noqa: E402

PIN = ["taskset", "-c", "0-7"]
DISPLAY = ":0"
PERIOD_NS = 150_000_000
MINIMUM = (400, 420)
# The History list point, as fractions of the window, checked in pilot frames on this host.
POINTS = {"1480x800": (0.30, 0.60), "461x490": (0.30, 0.78)}
WARMUP = [1] * 10 + [-1] * 10
RECORDED = ([1] * 30 + [-1] * 30) * 2
IDLE = [0] * 20


def now():
    return time.monotonic_ns()


class Pinned(session.Session):
    """Session.launch with the app under `taskset -c 0-7`; taskset execs the app, so the PID is the app's."""

    def launch(self):
        from native_qa import mutter

        self.require_unlocked("the launch")
        argv = PIN + [str(self.binary), str(self.fixture)]
        self.applog = open(self.dirs.root / "app.log", "wb")
        self.proc = subprocess.Popen(argv, env=self.env, stdout=self.applog, stderr=subprocess.STDOUT)
        self.log["launch"] = dict(argv=argv, pid=self.proc.pid, at_utc=session.utc(), input=self.input)
        dsp = x11.connect(self.env["DISPLAY"])
        window = x11.find_window(dsp, self.proc.pid)
        self.remote = mutter.RemoteDesktop.connect()
        self.driver = mutter.MutterDriver(dsp, window, self.log["input"], self.remote)
        self.size_window()
        time.sleep(self.settle)
        self.driver.activate()
        self.park("launch")


class Watch:
    """A second X connection: DamageNotify on the app window and XI2 raw input on the root, stamped on receipt."""

    def __init__(self, wid):
        from Xlib import X, display as xdisplay
        from Xlib.ext import damage, xinput

        self.X, self.xinput = X, xinput
        self.d = xdisplay.Display(DISPLAY)
        self.d.set_error_handler(lambda *a: None)
        self.d.damage_query_version()
        self.damage_event = self.d.query_extension("DAMAGE").first_event + damage.DamageNotifyCode
        self.xi_opcode = self.d.query_extension("XInputExtension").major_opcode
        self.d.xinput_query_version()
        self.win = self.d.create_resource_object("window", wid)
        self.win.damage_create(damage.DamageReportRawRectangles)
        self.d.screen().root.xinput_select_events(
            [(xinput.AllDevices, xinput.RawButtonPressMask | xinput.RawMotionMask)])
        self.d.sync()
        self.events = []  # (t_ns, kind, detail)

    def pump(self, deadline, until_damage=False):
        """Read events until `deadline`, or with `until_damage` until the first DamageNotify read in this call."""
        fd = self.d.fileno()
        while True:
            while self.d.pending_events():
                ev = self.d.next_event()
                t = now()
                if ev.type == self.damage_event:
                    self.events.append((t, "damage", [ev.area.x, ev.area.y, ev.area.width, ev.area.height]))
                    if until_damage:
                        return True
                elif ev.type == 35 and getattr(ev, "extension", None) == self.xi_opcode:
                    self.events.append((t, "raw", int(ev.evtype)))
            left = deadline - now()
            if left <= 0:
                return
            select.select([fd], [], [], left / 1e9)
            self.d.pending_events()  # reads what arrived

    def close(self):
        try:
            self.d.close()
        except Exception:
            pass


def schedstat(pid):
    exec_ns, wait_ns, slices = open(f"/proc/{pid}/task/{pid}/schedstat").read().split()
    return int(exec_ns), int(wait_ns), int(slices)


def where(pid):
    stat = open(f"/proc/{pid}/task/{pid}/stat").read()
    cpu = int(stat[stat.rfind(")") + 2:].split()[36])
    try:
        khz = int(open(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq").read())
    except OSError:
        khz = None
    return cpu, None if khz is None else round(khz / 1000)


def series(run, watch, pid, phase, directions, rows):
    """One notch (+1 down, -1 up) or idle window (0) every PERIOD_NS, each window closed by the next one's read."""
    remote = run.remote
    start = now() + 20_000_000
    watch.pump(start)
    prev = schedstat(pid)
    for i, direction in enumerate(directions):
        t_pre = start + i * PERIOD_NS
        watch.pump(t_pre)
        st = schedstat(pid)
        if rows and rows[-1]["phase"] == phase and "cpu_ms" not in rows[-1]:
            close_row(rows[-1], prev, st, pid)
        prev = st
        t_send = now()
        if direction:
            remote.wheel(direction)
        t_ret = now()
        row = dict(phase=phase, i=i, dir=direction, t_send=t_send, dbus_ms=round((t_ret - t_send) / 1e6, 3))
        rows.append(row)
        if direction and watch.pump(t_send + 40_000_000, until_damage=True):
            # The core the UI thread last ran on, read just after its frame reached the window: intel_pstate's
            # scaling_cur_freq then still reflects the busy interval.
            row["cpu_busy"], row["mhz_busy"] = where(pid)
    watch.pump(start + len(directions) * PERIOD_NS)
    close_row(rows[-1], prev, schedstat(pid), pid)


def close_row(row, before, after, pid):
    row["cpu_ms"] = round((after[0] - before[0]) / 1e6, 3)
    row["wait_ms"] = round((after[1] - before[1]) / 1e6, 3)
    row["slices"] = after[2] - before[2]
    row["cpu"], row["mhz"] = where(pid)


def attach_events(rows, events):
    """Each window's first raw input and first damage after its send, and its damage bursts."""
    events = sorted(events)
    for k, row in enumerate(rows):
        lo = row["t_send"]
        hi = rows[k + 1]["t_send"] if k + 1 < len(rows) else lo + PERIOD_NS
        inside = [e for e in events if lo <= e[0] < hi]
        raw = [e for e in inside if e[1] == "raw"]
        dmg = [e for e in inside if e[1] == "damage"]
        row["raw_ms"] = round((raw[0][0] - lo) / 1e6, 3) if raw else None
        row["raw_types"] = sorted({e[2] for e in raw})
        row["damage_ms"] = round((dmg[0][0] - lo) / 1e6, 3) if dmg else None
        row["damage_rect"] = dmg[0][2] if dmg else None
        frames, last = 0, None
        for e in dmg:
            if last is None or e[0] - last > 1_000_000:
                frames += 1
            last = e[0]
        row["frames"] = frames
        row["damage_events"] = len(dmg)


def measure(binary, fixture, size, run_dir, snapshot=False):
    width, height = (int(v) for v in size.split("x"))
    minimum = MINIMUM if (width, height) < (1000, 680) or width < 1000 or height < 680 else None
    prefs = stores.store_text("midnight", [], settings={"interface_text_size": 13})
    run = Pinned(Path(binary), Path(fixture), Path(run_dir), prefs, width=width, height=height, display=DISPLAY,
                 scale="1", backend="mutter", window_minimum=minimum)
    out = dict(binary=os.path.basename(binary), size=size, run_dir=str(run_dir), started_utc=session.utc(),
               load1_before=os.getloadavg()[0])
    watch = None
    rows = []
    try:
        run.launch()
        pid = run.proc.pid
        out["pid"] = pid
        out["exe_ok"] = os.readlink(f"/proc/{pid}/exe") == os.path.realpath(binary)
        out["affinity"] = sorted(os.sched_getaffinity(pid))
        assert out["exe_ok"] and out["affinity"] == list(range(8)), out
        out["window"] = list(run.driver.size())
        assert out["window"] == [width, height], out
        fx, fy = POINTS[size]
        px, py = int(width * fx), int(height * fy)
        out["point"] = [px, py]
        watch = Watch(run.driver.w.id)
        run.require_unlocked("the pointer move")
        run.driver.wheel(px, py, 0)  # glide there; refuses unless XWayland reports the pointer on the window there
        if snapshot:
            run.driver.snap().save(Path(run_dir) / "before.png")
        for phase, dirs in (("warmup", WARMUP), ("idle-before", IDLE), ("recorded", RECORDED), ("idle-after", IDLE)):
            run.require_unlocked(f"the {phase} phase")
            if not run.driver.on_window(px, py):
                raise SystemExit(f"pointer left the History point before {phase}")
            series(run, watch, pid, phase, dirs, rows)
            if snapshot and phase == "warmup":
                run.driver.snap().save(Path(run_dir) / "after-warmup.png")
            if snapshot and phase == "recorded":
                run.driver.snap().save(Path(run_dir) / "after-recorded.png")
        if snapshot:  # pilot only: show that the notches scroll the list
            extra = []
            series(run, watch, pid, "pilot-scroll", [1] * 10, extra)
            time.sleep(0.5)
            run.driver.snap().save(Path(run_dir) / "scrolled-10-down.png")
        attach_events(rows, watch.events)
        out["raw_event_types"] = sorted({e[2] for e in watch.events if e[1] == "raw"})
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


def main():
    cmd = sys.argv[1]
    if cmd == "pilot":
        binary, fixture, size, outdir = sys.argv[2:6]
        res = measure(os.path.abspath(binary), os.path.abspath(fixture), size, os.path.abspath(outdir), snapshot=True)
        json.dump(res, open(Path(outdir) / "pilot.json", "w"), indent=1)
        rec = [r for r in res["rows"] if r["phase"] == "recorded"]
        idle = [r for r in res["rows"] if r["phase"].startswith("idle")]
        print(json.dumps({k: res[k] for k in res if k != "rows"}))
        for name, rs in (("recorded", rec), ("idle", idle)):
            for m in ("damage_ms", "raw_ms", "cpu_ms", "wait_ms", "frames", "dbus_ms"):
                vals = [r[m] for r in rs if r[m] is not None]
                print(name, m, len(vals), "p50", pct(vals, 50), "p95", pct(vals, 95), "max", max(vals) if vals else None)
        print("no-damage notches", [r["i"] for r in rec if r["damage_ms"] is None])
        return
    if cmd == "run":
        base, cand, fixture, out, plan = sys.argv[2:7]
        base, cand, fixture = (os.path.abspath(p) for p in (base, cand, fixture))
        launches = [(item[0], item[1:]) for item in plan.split(",")]
        root = Path(f"/tmp/gitturtle-evidence/runs/history-columns-steady-{int(time.time())}")
        root.mkdir(parents=True)
        meta = {"host": host(), "builds": {"base": build(base), "cand": build(cand)}, "fixture": fixture,
                "fixture_head": session.git(Path(fixture), "rev-parse", "HEAD"), "plan": plan,
                "pinning": " ".join(PIN), "period_ms": PERIOD_NS / 1e6, "points": POINTS,
                "phases": {"warmup": WARMUP, "idle": len(IDLE), "recorded": RECORDED}, "window_minimum": MINIMUM,
                "percentile": "nearest rank", "started_utc": session.utc(), "load_before": os.getloadavg(),
                "driver_sha256": hashlib.sha256(open(__file__, "rb").read()).hexdigest()}
        results = []
        for n, (who, size) in enumerate(launches):
            binary = base if who == "B" else cand
            res = measure(binary, fixture, size, root / f"{n:02d}-{who}-{size}")
            res.update(n=n, build="base" if who == "B" else "cand")
            results.append(res)
            rec = [r for r in res["rows"] if r["phase"] == "recorded"]
            dm = [r["damage_ms"] for r in rec if r["damage_ms"] is not None]
            cp = [r["cpu_ms"] for r in rec]
            print(json.dumps({"n": n, "build": res["build"], "size": size, "damage_p50": pct(dm, 50),
                              "cpu_p50": pct(cp, 50), "missing": len(rec) - len(dm), "load1": res["load1_before"],
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


def boot(groups_a, groups_b, stat, n=10000, seed=1):
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
    # name: (row value, row filter)
    "damage_ms": (lambda r: r["damage_ms"], lambda r: True),
    "app_ms": (lambda r: None if r["damage_ms"] is None or r["raw_ms"] is None else round(r["damage_ms"] - r["raw_ms"], 3),
               lambda r: True),
    "cpu_ms": (lambda r: r["cpu_ms"], lambda r: True),
    "damage_ms_1f": (lambda r: r["damage_ms"], lambda r: r["frames"] == 1),
    "cpu_ms_1f": (lambda r: r["cpu_ms"], lambda r: r["frames"] == 1),
    "raw_ms": (lambda r: r["raw_ms"], lambda r: True),
    "wait_ms": (lambda r: r["wait_ms"], lambda r: True),
}
DELTAS = ("damage_ms", "app_ms", "cpu_ms", "damage_ms_1f", "cpu_ms_1f")


def cell_stats(groups):
    allv = [v for g in groups for v in g]
    return {"n": len(allv), "p50": pct(allv, 50), "p95": pct(allv, 95), "p99": pct(allv, 99),
            "max": max(allv) if allv else None, "launch_p50": [pct(g, 50) for g in groups],
            "launch_p95": [pct(g, 95) for g in groups]}


def analyze(path):
    data = json.load(open(path))
    summary = {}
    sizes = sorted({l["size"] for l in data["launches"]}, key=lambda s: -int(s.split("x")[0]))
    for size in sizes:
        by = {b: [l for l in data["launches"] if l["size"] == size and l["build"] == b] for b in ("base", "cand")}
        for name, (value, keep) in METRICS.items():
            groups = {b: [[value(r) for r in l["rows"] if r["phase"] == "recorded" and keep(r) and value(r) is not None]
                          for l in ls] for b, ls in by.items()}
            cell = {b: cell_stats(g) for b, g in groups.items()}
            if name in DELTAS:
                for p in (50, 95):
                    d = round(cell["cand"][f"p{p}"] - cell["base"][f"p{p}"], 3)
                    cell[f"delta_p{p}"] = {"delta": d, "ci95": boot(groups["base"], groups["cand"],
                                                                    lambda v, p=p: pct(v, p), n=int(os.environ.get("BOOT_N", "4000")))}
            summary[f"{size} {name}"] = cell
            print(size, name, json.dumps({k: v for k, v in cell.items()}), flush=True)
        for b, ls in by.items():
            rows = [r for l in ls for r in l["rows"]]
            idle = [r for r in rows if r["phase"].startswith("idle")]
            rec = [r for r in rows if r["phase"] == "recorded"]
            summary[f"{size} {b} counts"] = {
                "launches": len(ls), "recorded": len(rec), "no_frame": sum(r["frames"] == 0 for r in rec),
                "two_frames": sum(r["frames"] >= 2 for r in rec),
                "two_frame_indexes": sorted({r["i"] for r in rec if r["frames"] >= 2}),
                "idle_windows": len(idle), "idle_frames": sum(r["frames"] for r in idle),
                "idle_cpu_p50": pct([r["cpu_ms"] for r in idle], 50), "idle_cpu_p95": pct([r["cpu_ms"] for r in idle], 95),
                "idle_cpu_max": max(r["cpu_ms"] for r in idle),
                "idle_cpu_quiet_p50": pct([r["cpu_ms"] for r in idle if r["frames"] == 0], 50),
                "mhz_busy_p50": pct([r["mhz_busy"] for r in rec if r.get("mhz_busy")], 50),
                "mhz_busy_p5": pct([r["mhz_busy"] for r in rec if r.get("mhz_busy")], 5),
                "mhz_busy_max": max([r["mhz_busy"] for r in rec if r.get("mhz_busy")], default=None),
                "cpus_busy": sorted({r["cpu_busy"] for r in rec if r.get("cpu_busy") is not None}),
                "load1_before": [l["load1_before"] for l in ls],
                "exits": [l["exit"] for l in ls], "fixture_unchanged": all(l["fixture_unchanged"] for l in ls)}
            print(size, b, json.dumps(summary[f"{size} {b} counts"]), flush=True)
    data["summary"] = summary
    json.dump(data, open(path, "w"))


def export(path, csv_out, json_out):
    """The raw rows as CSV, and everything else (host, builds, plan, per-launch metadata, summary) as JSON."""
    data = json.load(open(path))
    cols = ["launch", "build", "size", "phase", "i", "dir", "t_ms", "dbus_ms", "raw_ms", "damage_ms", "frames",
            "damage_events", "cpu_ms", "wait_ms", "slices", "cpu", "mhz", "cpu_busy", "mhz_busy"]
    with open(csv_out, "w") as f:
        f.write(",".join(cols) + "\n")
        for l in data["launches"]:
            for r in l["rows"]:
                vals = [l["n"], l["build"], l["size"]] + [r.get(c) for c in cols[3:]]
                f.write(",".join("" if v is None else str(v) for v in vals) + "\n")
    meta = {k: v for k, v in data.items() if k != "launches"}
    meta["launches"] = [{k: v for k, v in l.items() if k != "rows"} for l in data["launches"]]
    for l in meta["launches"]:
        l["run_dir"] = os.path.basename(l["run_dir"])
    for b in meta["builds"].values():
        b["build_info"] = {k: v for k, v in (b.get("build_info") or {}).items()}
    meta["fixture"] = "/tmp/gitturtle-evidence/history-columns-steady-after-toggle/history"
    json.dump(meta, open(json_out, "w"), indent=1)


if __name__ == "__main__":
    main()
