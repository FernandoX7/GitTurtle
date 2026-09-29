#!/usr/bin/env python3
"""Launch timing of the desktop code font setting, cold and warm fontconfig caches (Hyprland).

Usage: cold_launch.py BINARY FIXTURE OUT_JSON [ROUNDS]

Run it from a Hyprland session. It creates a temporary headless output named QA (1480x800 at
scale 1, placed at 3000x0), focuses a spare workspace on it so every launch maps there, and
removes the output and reloads Hyprland's configuration at the end. It never touches the
caller's own GitTurtle data, fontconfig cache or Omarchy state.

Each launch gets a fresh HOME and XDG directories (so fontconfig's user cache is always new), the
GitTurtle QA identity, TZ=UTC and a preferences file with `system_code_font` on or off. "cold"
also points FONTCONFIG_FILE at a copy of /etc/fonts/fonts.conf whose <cachedir>s are replaced by
one empty directory, so fc-match must rescan every font; "warm" uses the system configuration and
its populated /var/cache/fontconfig. The kernel page cache stays warm in both (no drop_caches).

Timestamps are CLOCK_REALTIME in microseconds:
- t0: just before the process is spawned.
- window: Hyprland's socket2 `openwindow` event for the app's class (the window is mapped; its
  first frame may come later).
- lookup: an `fc-match` wrapper first on PATH logs the start and end of every call the app makes;
  the `monospace` call is the code font lookup (only with the setting on), and its end is the
  earliest moment the family can apply. The frame that applies it is not observed. The wrapper
  is bash (EPOCHREALTIME) around /usr/bin/fc-match, which adds its own start-up to each call.
Rounds interleave the four configurations in a rotating order. The app is stopped with SIGTERM
only. Percentiles are by nearest rank."""
import hashlib, json, os, platform, re, socket, subprocess, sys, time

BIN, FIX, OUT = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2]), sys.argv[3]
ROUNDS = int(sys.argv[4]) if len(sys.argv) > 4 else 10
ROOT = f"/tmp/gitturtle-evidence/runs/cold-launch-{int(time.time())}"
CLASS = "com.gitturtle.desktop"
OUTPUT, MODE, WORKSPACE = "QA", "1480x800", "9"
WRAPPER = """#!/bin/bash
s=$EPOCHREALTIME
printf 'S %s %s %s\\n' "$s" "$$" "$*" >> "$FCLOG"
# fc-match dies with this wrapper, as it would with the app's own child when the app kills
# a lookup that outlives its bound; an orphan would keep rescanning and warm the cache.
out=$(setpriv --pdeathsig KILL /usr/bin/fc-match "$@"); rc=$?
e=$EPOCHREALTIME
printf 'E %s %s %s %s\\n' "$s" "$e" "$rc" "$*" >> "$FCLOG"
printf '%s' "$out"
exit $rc
"""


def now_us():
    return time.time_ns() // 1000


def hypr(*args):
    return subprocess.run(["hyprctl", *args], capture_output=True, text=True).stdout


def with_output(body):
    """Runs body() with a temporary headless output focused, and always removes it."""
    hypr("output", "create", "headless", OUTPUT)
    time.sleep(1)
    hypr("eval", f'hl.monitor({{ output = "{OUTPUT}", mode = "{MODE}@60", position = "3000x0", scale = 1 }})')
    time.sleep(1)
    try:
        hypr("dispatch", f'hl.dsp.focus({{ monitor = "{OUTPUT}" }})')
        hypr("dispatch", f'hl.dsp.focus({{ workspace = "{WORKSPACE}" }})')
        time.sleep(0.8)
        active = json.loads(hypr("activeworkspace", "-j"))
        assert active.get("monitor") == OUTPUT and active.get("name") == WORKSPACE, active
        return body()
    finally:
        hypr("output", "remove", OUTPUT)
        time.sleep(1)
        hypr("reload")
        time.sleep(1.5)


def cold_conf(path, cache):
    conf = open("/etc/fonts/fonts.conf").read()
    conf = re.sub(r"<cachedir[^>]*>.*?</cachedir>\s*", "", conf, flags=re.S)
    conf = conf.replace("</fontconfig>", f"<cachedir>{cache}</cachedir>\n</fontconfig>")
    open(path, "w").write(conf)


def events():
    sig = os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(f"{os.environ['XDG_RUNTIME_DIR']}/hypr/{sig}/.socket2.sock")
    s.settimeout(0.05)
    return s


def wait_window(sock, deadline):
    buf = b""
    while time.time() < deadline:
        try:
            chunk = sock.recv(65536)
        except socket.timeout:
            continue
        t = now_us()
        buf += chunk
        *lines, buf = buf.split(b"\n")
        for line in lines:
            if line.startswith(b"openwindow>>") and f",{CLASS},".encode() in line:
                return t, line.decode(errors="replace")
    return None, None


def fc_log(path):
    """(finished calls as [start, end, exit, args], calls still running, calls that never
    finished: the app killed their wrapper when the lookup outlived its bound)."""
    lines = open(path).read().splitlines() if os.path.exists(path) else []
    done = [l.split(" ", 4)[1:] for l in lines if l.startswith("E ")]
    ended = {c[0] for c in done}
    started = [l.split(" ", 3)[1:] for l in lines if l.startswith("S ")]
    open_calls = [c for c in started if c[0] not in ended]
    running = sum(os.path.exists(f"/proc/{c[1]}") for c in open_calls)
    return done, running, [{"start": c[0], "args": c[2]} for c in open_calls]


def launch(run, font_on, cold):
    for d in ("home", "config/gitturtle", "data", "cache", "state", "bin"):
        os.makedirs(f"{run}/{d}")
    open(os.path.join(run, "home", ".gitconfig"), "w").write("[user]\n\tname = GitTurtle QA\n\temail = qa@example.invalid\n")
    prefs = {"version": 6, "settings": {"theme": "midnight", "follow_system": False, "project_pane": False,
                                        "system_code_font": font_on}, "recent_repositories": [FIX], "project_library": []}
    open(f"{run}/config/gitturtle/preferences.json", "w").write(json.dumps(prefs))
    open(f"{run}/bin/fc-match", "w").write(WRAPPER)
    os.chmod(f"{run}/bin/fc-match", 0o755)
    env = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DEBUG", "GIT_EDITOR", "FONTCONFIG_FILE")}
    env.update(TZ="UTC", HOME=f"{run}/home", XDG_CONFIG_HOME=f"{run}/config", XDG_DATA_HOME=f"{run}/data",
               XDG_CACHE_HOME=f"{run}/cache", XDG_STATE_HOME=f"{run}/state", FCLOG=f"{run}/fc.log",
               PATH=f"{run}/bin:{os.environ['PATH']}")
    if cold:
        os.makedirs(f"{run}/fc-cache")
        cold_conf(f"{run}/fonts.conf", f"{run}/fc-cache")
        env["FONTCONFIG_FILE"] = f"{run}/fonts.conf"
    sock = events()
    t0 = now_us()
    proc = subprocess.Popen([BIN, FIX], cwd=run, env=env, stdin=subprocess.DEVNULL,
                            stdout=open(f"{run}/stdout.log", "w"), stderr=open(f"{run}/stderr.log", "w"),
                            start_new_session=True)
    window, event = wait_window(sock, time.time() + 30)
    sock.close()
    # The lookup starts after the launch snapshot. Wait at least 3 s after the window, then
    # for every started fc-match call to finish (a cold rescan can take seconds), up to 60 s,
    # and with the setting on for the monospace lookup itself.
    settle, deadline = time.time() + 3.0, time.time() + 60.0
    while time.time() < deadline:
        calls, running, _ = fc_log(env["FCLOG"])
        mono = [c for c in calls if c[3].endswith("monospace")]
        if time.time() >= settle and running == 0 and (mono or not font_on):
            break
        time.sleep(0.05)
    time.sleep(0.5)
    calls, running, killed = fc_log(env["FCLOG"])
    mono = [c for c in calls if c[3].endswith("monospace")]
    lookup = mono[0] if mono else None
    assert os.readlink(f"/proc/{proc.pid}/exe") == os.path.realpath(BIN)
    workspace = event.split(">>", 1)[1].split(",")[1] if event else None
    proc.terminate()
    proc.wait(timeout=10)
    us = lambda s: int(float(s) * 1e6)
    return {
        "setting": "on" if font_on else "off", "fontconfig": "cold" if cold else "warm", "run": run,
        "workspace": workspace,
        "window_ms": None if window is None else (window - t0) / 1000,
        "lookup_start_ms": None if lookup is None else (us(lookup[0]) - t0) / 1000,
        "lookup_end_ms": None if lookup is None else (us(lookup[1]) - t0) / 1000,
        "lookup_exit": None if lookup is None else int(lookup[2]),
        "calls": [{"start_ms": (us(c[0]) - t0) / 1000, "end_ms": (us(c[1]) - t0) / 1000, "exit": int(c[2]), "args": c[3]} for c in calls],
        "unfinished": [{"start_ms": (us(k["start"]) - t0) / 1000, "args": k["args"]} for k in killed],
        "still_running": running, "exit": proc.returncode,
    }


def pct(values, p):
    v = sorted(values)
    return v[max(0, -(-len(v) * p // 100) - 1)] if v else None


def host():
    cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None)
    gov = open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").read().strip() \
        if os.path.exists("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor") else None
    fc = subprocess.run(["/usr/bin/fc-match", "--version"], capture_output=True, text=True)
    return {"kernel": platform.release(), "cpu": cpu, "cores": os.cpu_count(), "governor": gov,
            "fontconfig": " ".join(fc.stdout.split() + fc.stderr.split()),
            "hyprland": (hypr("version").splitlines() or [None])[0]}


def build():
    digest = hashlib.sha256(open(BIN, "rb").read()).hexdigest()
    info = subprocess.run([BIN, "--build-info"], capture_output=True, text=True).stdout.strip()
    return {"binary": BIN, "sha256": digest, "build_info": info}


def main():
    configs = [(True, True), (False, True), (True, False), (False, False)]
    samples = []
    os.makedirs(ROOT)
    meta = {"host": host(), "build": build(), "fixture": FIX, "rounds": ROUNDS, "output": f"{OUTPUT} {MODE} scale 1",
            "percentile": "nearest rank", "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "load_before": os.getloadavg()}

    def rounds():
        for r in range(ROUNDS):
            order = configs[r % 4:] + configs[:r % 4]
            for font_on, cold in order:
                run = f"{ROOT}/r{r:02d}-{'on' if font_on else 'off'}-{'cold' if cold else 'warm'}"
                load = os.getloadavg()[0]
                s = launch(run, font_on, cold)
                s.update(round=r, load1_before=load, load1_after=os.getloadavg()[0])
                samples.append(s)
                print(json.dumps({k: s[k] for k in ("round", "setting", "fontconfig", "window_ms", "lookup_end_ms",
                                                    "workspace", "load1_before")}), flush=True)
                time.sleep(1.0)

    with_output(rounds)
    meta.update(ended_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), load_after=os.getloadavg())
    summary = {}
    for font_on, cold in configs:
        key = f"{'on' if font_on else 'off'}-{'cold' if cold else 'warm'}"
        sel = [s for s in samples if s["setting"] == ("on" if font_on else "off") and s["fontconfig"] == ("cold" if cold else "warm")]
        out = {}
        for metric in ("window_ms", "lookup_end_ms"):
            vals = [s[metric] for s in sel if s[metric] is not None]
            if vals:
                out[metric] = {"n": len(vals), "p50": pct(vals, 50), "p95": pct(vals, 95), "max": max(vals)}
        summary[key] = out
    json.dump({**meta, "summary": summary, "samples": samples}, open(OUT, "w"), indent=1)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
