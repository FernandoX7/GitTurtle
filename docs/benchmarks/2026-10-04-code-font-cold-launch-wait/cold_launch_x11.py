#!/usr/bin/env python3
"""Launch timing of the desktop code font, cold and warm fontconfig caches, base against candidate (X11).

Usage: cold_launch_x11.py BASE_BINARY CAND_BINARY FIXTURE OUT_JSON [ROUNDS]

The X11 counterpart of docs/benchmarks/2026-09-29-code-font-cold-launch/cold_launch.py, for an
XWayland display (DISPLAY, default :0) under GNOME. Needs python-xlib with the DAMAGE extension.
It never touches the caller's GitTurtle data or fontconfig cache and sends no input.

Every launch runs `taskset -c 0-7 BINARY FIXTURE` with the setting on, a fresh HOME and XDG
directories, the GitTurtle QA identity, TZ=UTC, Midnight, WAYLAND_DISPLAY, GIT_* and
FONTCONFIG_FILE removed, and GPUI_X11_SCALE_FACTOR=1.
- cold: FONTCONFIG_FILE names a copy of /etc/fonts/fonts.conf whose conf.d include is made
  absolute and whose only <cachedir> is the launch's fresh, absent XDG_CACHE_HOME/fontconfig,
  so fc-match must scan every font directory.
- warm: the stock configuration and the host's /var/cache/fontconfig.

Timestamps are CLOCK_REALTIME microseconds from t0, the driver's stamp just before Popen (so they
include Python's fork and exec and taskset's exec):
- window: receipt of the MapNotify of the app's top-level window (_NET_WM_PID = the launch's
  PID). GPUI maps it at the end of window construction, before its first frame.
- frame: receipt of the second DamageNotify (raw rectangles) on that window. Composite damages
  the whole window when it maps (the first event, at the map); the window has no background, so
  the next damage is the app's first presented frame. Presentation on the output is excluded.
- lookup: an `fc-match` wrapper first on PATH logs the start, end, exit and output of every call;
  the end of the first `monospace` call that exits 0 is the earliest moment the family can apply.
- activation: receipt of the root _NET_ACTIVE_WINDOW change that names the app's window.
Rounds run the four cells (build x cache) once, in an order that rotates by round. One warm
launch of each build precedes the rounds and is kept apart. Each launch waits at least 3 s after
the window, then until no fc-match call is running and a monospace call has exited 0, up to 12 s
after the window. The app is stopped with SIGTERM. Percentiles are by nearest rank."""
import hashlib, json, os, platform, re, select, subprocess, sys, time
from Xlib import X, display as xdisplay, error as xerror
from Xlib.ext import damage

BASE, CAND, FIX, OUT = (os.path.abspath(p) for p in sys.argv[1:5])
ROUNDS = int(sys.argv[5]) if len(sys.argv) > 5 else 24
DISPLAY = os.environ.get("QA_DISPLAY", ":0")
ROOT = f"/tmp/gitturtle-evidence/runs/cold-launch-x11-{int(time.time())}"
PIN = ["taskset", "-c", "0-7"]
SETTLE, OBSERVE = 3.0, 12.0
WRAPPER = """#!/bin/bash
s=$EPOCHREALTIME
printf 'S %s %s %s\\n' "$s" "$$" "$*" >> "$FCLOG"
# fc-match dies with this wrapper, as the app's own child does when the app kills a lookup.
out=$(setpriv --pdeathsig KILL /usr/bin/fc-match "$@"); rc=$?
e=$EPOCHREALTIME
printf 'E %s %s %s %s\\n' "$s" "$e" "$rc" "$*" >> "$FCLOG"
printf 'O %s %q\\n' "$s" "$out" >> "$FCLOG"
printf '%s' "$out"
exit $rc
"""


def now_us():
    return time.time_ns() // 1000


class Screen:
    def __init__(self):
        self.d = xdisplay.Display(DISPLAY)
        self.d.set_error_handler(lambda *a: None)
        self.d.damage_query_version()
        self.damage_event = self.d.query_extension("DAMAGE").first_event + damage.DamageNotifyCode
        self.root = self.d.screen().root
        self.root.change_attributes(event_mask=X.SubstructureNotifyMask | X.PropertyChangeMask)
        self.pid_atom = self.d.intern_atom("_NET_WM_PID")
        self.active_atom = self.d.intern_atom("_NET_ACTIVE_WINDOW")
        self.d.sync()

    def reset(self):
        self.created, self.mapped, self.damaged, self.active = {}, {}, {}, []
        while self.d.pending_events():
            self.d.next_event()

    def pump(self, timeout):
        if not self.d.pending_events():
            select.select([self.d.fileno()], [], [], timeout)
        while self.d.pending_events():
            ev = self.d.next_event()
            t = now_us()
            if ev.type == X.CreateNotify and ev.parent == self.root:
                self.created.setdefault(ev.window.id, t)
                try:
                    ev.window.change_attributes(event_mask=X.StructureNotifyMask)
                    ev.window.damage_create(damage.DamageReportRawRectangles)
                except xerror.XError:
                    pass
            elif ev.type == X.MapNotify:
                self.mapped.setdefault(ev.window.id, t)
            elif ev.type == self.damage_event:
                seen = self.damaged.setdefault(ev.drawable.id, [])
                if len(seen) < 8:
                    seen.append((t, ev.area.width, ev.area.height))
            elif ev.type == X.PropertyNotify and ev.atom == self.active_atom:
                prop = self.root.get_full_property(self.active_atom, X.AnyPropertyType)
                self.active.append((t, prop.value[0] if prop and len(prop.value) else 0))
        self.d.flush()

    def pid_of(self, wid):
        try:
            prop = self.d.create_resource_object("window", wid).get_full_property(self.pid_atom, X.AnyPropertyType)
            return prop.value[0] if prop else None
        except xerror.XError:
            return None


def cold_conf(path, cache):
    conf = open("/etc/fonts/fonts.conf").read()
    conf = re.sub(r"<cachedir[^>]*>.*?</cachedir>\s*", "", conf, flags=re.S)
    conf = conf.replace(">conf.d</include>", ">/etc/fonts/conf.d</include>")
    conf = conf.replace("</fontconfig>", f"<cachedir>{cache}</cachedir>\n</fontconfig>")
    open(path, "w").write(conf)


def cache_state(path):
    files = [os.path.join(path, f) for f in os.listdir(path)] if os.path.isdir(path) else []
    return {"exists": os.path.isdir(path), "files": len(files), "bytes": sum(os.path.getsize(f) for f in files)}


def fc_log(path):
    lines = open(path).read().splitlines() if os.path.exists(path) else []
    outs = {l.split(" ", 2)[1]: l.split(" ", 2)[2] if l.count(" ") >= 2 else "" for l in lines if l.startswith("O ")}
    done = [l.split(" ", 5)[1:] for l in lines if l.startswith("E ")]
    ended = {c[0] for c in done}
    started = [l.split(" ", 3)[1:] for l in lines if l.startswith("S ")]
    open_calls = [c for c in started if c[0] not in ended]
    running = sum(os.path.exists(f"/proc/{c[1]}") for c in open_calls)
    return done, outs, running, open_calls


def cpu_mhz():
    vals, cpu = {}, None
    for line in open("/proc/cpuinfo"):
        if line.startswith("processor"):
            cpu = int(line.split(":")[1])
        elif line.startswith("cpu MHz") and cpu is not None and cpu < 8:
            vals[cpu] = round(float(line.split(":")[1]))
    return vals


def launch(scr, run, binary, cold):
    for d in ("home", "config/gitturtle", "data", "cache", "state", "bin"):
        os.makedirs(f"{run}/{d}")
    open(os.path.join(run, "home", ".gitconfig"), "w").write("[user]\n\tname = GitTurtle QA\n\temail = qa@example.invalid\n")
    prefs = {"version": 6, "settings": {"theme": "midnight", "follow_system": False, "system_code_font": True},
             "recent_repositories": [FIX], "project_library": [{"project": {"path": FIX}}]}
    open(f"{run}/config/gitturtle/preferences.json", "w").write(json.dumps(prefs))
    open(f"{run}/bin/fc-match", "w").write(WRAPPER)
    os.chmod(f"{run}/bin/fc-match", 0o755)
    env = {k: v for k, v in os.environ.items()
           if k not in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "FONTCONFIG_FILE", "GIT_EDITOR") and not k.startswith("GIT_")}
    env.update(TZ="UTC", DISPLAY=DISPLAY, GPUI_X11_SCALE_FACTOR="1", HOME=f"{run}/home",
               XDG_CONFIG_HOME=f"{run}/config", XDG_DATA_HOME=f"{run}/data", XDG_CACHE_HOME=f"{run}/cache",
               XDG_STATE_HOME=f"{run}/state", FCLOG=f"{run}/fc.log", PATH=f"{run}/bin:{os.environ['PATH']}")
    user_cache = f"{run}/cache/fontconfig"
    if cold:
        cold_conf(f"{run}/fonts.conf", user_cache)
        env["FONTCONFIG_FILE"] = f"{run}/fonts.conf"
    before = cache_state(user_cache)
    assert not before["exists"], before
    mhz, load = cpu_mhz(), os.getloadavg()[0]
    scr.reset()
    t0 = now_us()
    proc = subprocess.Popen(PIN + [binary, FIX], cwd=run, env=env, stdin=subprocess.DEVNULL,
                            stdout=open(f"{run}/stdout.log", "w"), stderr=open(f"{run}/stderr.log", "w"),
                            start_new_session=True)
    window = wid = None
    deadline = time.time() + 30
    while window is None and time.time() < deadline:
        scr.pump(0.05)
        for w, seen in sorted(scr.damaged.items(), key=lambda kv: kv[1][0][0]):
            if scr.pid_of(w) == proc.pid:
                window, wid = seen[0][0], w
                break
    t_window = time.time()
    settle, stop = t_window + SETTLE, t_window + OBSERVE
    while time.time() < stop:
        scr.pump(0.05)
        calls, _, running, _ = fc_log(env["FCLOG"])
        ok = [c for c in calls if c[4].endswith("monospace") and c[2] == "0"]
        if time.time() >= settle and running == 0 and ok:
            break
    time.sleep(0.3)
    scr.pump(0.0)
    calls, outs, running, unfinished = fc_log(env["FCLOG"])
    assert os.readlink(f"/proc/{proc.pid}/exe") == os.path.realpath(binary)
    affinity = sorted(os.sched_getaffinity(proc.pid))
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    us = lambda s: int(float(s) * 1e6)
    rel = lambda t: None if t is None else round((t - t0) / 1000, 3)
    mono_ok = [c for c in calls if c[4].endswith("monospace") and c[2] == "0"]
    first = mono_ok[0] if mono_ok else None
    family = outs.get(first[0]) if first else None
    activated = [t for t, w in scr.active if wid is not None and w == wid]
    return {
        "build": "cand" if binary == CAND else "base", "fontconfig": "cold" if cold else "warm", "run": run,
        "window_ms": rel(scr.mapped.get(wid)) if wid else None,
        "frame_ms": rel(scr.damaged[wid][1][0]) if wid and len(scr.damaged.get(wid, [])) > 1 else None,
        "damage_ms": [(rel(t), w, h) for t, w, h in scr.damaged.get(wid, [])],
        "activated_ms": rel(activated[0]) if activated else None,
        "family_ms": None if first is None else rel(us(first[1])),
        "family_lookup_start_ms": None if first is None else rel(us(first[0])),
        "family": family, "applied": first is not None and (family or "").startswith("DejaVu"),
        "calls": [{"start_ms": rel(us(c[0])), "end_ms": rel(us(c[1])), "exit": int(c[2]), "args": c[4],
                   "output": outs.get(c[0])} for c in calls],
        "unfinished": [{"start_ms": rel(us(c[0])), "args": c[2]} for c in unfinished],
        "still_running": running, "exit": proc.returncode, "affinity": affinity,
        "cache_before": before, "cache_after": cache_state(user_cache),
        "cpu_mhz_0_7": mhz, "load1_before": load,
    }


def pct(values, p):
    v = sorted(values)
    return v[max(0, -(-len(v) * p // 100) - 1)] if v else None


def host():
    cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None)
    gov = open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").read().strip()
    fc = subprocess.run(["/usr/bin/fc-match", "--version"], capture_output=True, text=True)
    sysc = "/var/cache/fontconfig"
    return {"kernel": platform.release(), "cpu": cpu, "logical_cpus": os.cpu_count(), "governor": gov,
            "fontconfig": " ".join(fc.stdout.split() + fc.stderr.split()), "display": DISPLAY,
            "session": os.environ.get("XDG_SESSION_TYPE"), "desktop": os.environ.get("XDG_CURRENT_DESKTOP"),
            "system_fontconfig_cache": cache_state(sysc)}


def build(b):
    digest = hashlib.sha256(open(b, "rb").read()).hexdigest()
    info = json.loads(subprocess.run([b, "--build-info"], capture_output=True, text=True).stdout)
    return {"binary": b, "sha256": digest, "build_info": info}


def main():
    cells = [(CAND, True), (BASE, True), (CAND, False), (BASE, False)]
    os.makedirs(ROOT)
    scr = Screen()
    meta = {"host": host(), "builds": {"base": build(BASE), "cand": build(CAND)}, "fixture": FIX, "rounds": ROUNDS,
            "pinning": " ".join(PIN), "percentile": "nearest rank",
            "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "load_before": os.getloadavg()}
    warmup, samples = [], []
    for binary in (BASE, CAND):
        s = launch(scr, f"{ROOT}/warmup-{'cand' if binary == CAND else 'base'}", binary, False)
        warmup.append(s)
        print(json.dumps({"warmup": s["build"], "window_ms": s["window_ms"], "family_ms": s["family_ms"]}), flush=True)
        time.sleep(1.0)
    for r in range(ROUNDS):
        order = cells[r % 4:] + cells[:r % 4]
        for binary, cold in order:
            name = f"r{r:02d}-{'cand' if binary == CAND else 'base'}-{'cold' if cold else 'warm'}"
            s = launch(scr, f"{ROOT}/{name}", binary, cold)
            s.update(round=r, load1_after=os.getloadavg()[0])
            samples.append(s)
            print(json.dumps({k: s[k] for k in ("round", "build", "fontconfig", "window_ms", "frame_ms", "family_ms", "applied",
                                                "activated_ms", "load1_before")}), flush=True)
            time.sleep(1.0)
    meta.update(ended_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), load_after=os.getloadavg())
    summary = {}
    for binary, cold in cells:
        key = f"{'cand' if binary == CAND else 'base'}-{'cold' if cold else 'warm'}"
        sel = [s for s in samples if s["build"] == key.split("-")[0] and s["fontconfig"] == key.split("-")[1]]
        out = {"launches": len(sel), "applied": sum(s["applied"] for s in sel)}
        for metric in ("window_ms", "frame_ms", "family_ms"):
            vals = [s[metric] for s in sel if s[metric] is not None]
            if vals:
                out[metric] = {"n": len(vals), "p50": pct(vals, 50), "p95": pct(vals, 95), "max": max(vals)}
        summary[key] = out
    json.dump({**meta, "summary": summary, "warmup": warmup, "samples": samples}, open(OUT, "w"), indent=1)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
