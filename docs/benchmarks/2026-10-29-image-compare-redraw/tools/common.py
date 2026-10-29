"""common.py: one isolated launch of a GitTurtle executable on a fresh copy of
fixtures/pristine/FIXTURE, as in docs/benchmarks/2026-10-29-changes-body-redraw/tools/drive.py:
taskset -c 4-11, empty absolute XDG homes under cfg/LABEL (fontconfig linked, preferences from
the native-QA template: Midnight, follow-system off, interface text SIZE pt, project pane off),
floating W x H, centred, opaque, on workspace 9. measured=True sets NO_AT_BRIDGE=1 and
WAYLAND_DEBUG=1 (log to raw/LABEL.wl.log); otherwise the accessibility bridge stays on."""
import json, os, signal, subprocess, sys, time, atexit
HERE = os.path.dirname(os.path.abspath(__file__)); TOP = os.path.dirname(HERE)
QA_TOOLS = os.environ.get('GITTURTLE_QA_TOOLS', os.path.join(TOP, '..', '..', '..', '.local', 'item8-2026-10-27', 'tools'))
VPTR = os.path.join(QA_TOOLS, 'vptr/vptr'); FIND = os.path.join(QA_TOOLS, 'atspi_find.py')
TEMPLATE = os.path.join(QA_TOOLS, 'prefs-template.json')
def sh(*a): return subprocess.run(a, check=True, capture_output=True, text=True).stdout
def hypr(cmd): sh('hyprctl', 'dispatch', cmd)
class Launch:
    def __init__(self, binary, fixture, size, label, w, h, measured):
        self.label, self.w, self.h = label, w, h
        run = f'{TOP}/fixtures/run'; repo = f'{run}/{fixture}'; os.makedirs(run, exist_ok=True)
        subprocess.run(['rm', '-rf', repo], check=True); subprocess.run(['cp', '-a', f'{TOP}/fixtures/pristine/{fixture}', repo], check=True)
        cfg = f'{TOP}/cfg/{label}'; subprocess.run(['rm', '-rf', cfg], check=True)
        for d in ('gitturtle', 'data', 'state', 'cache'): os.makedirs(f'{cfg}/{d}')
        os.symlink(os.path.expanduser('~/.config/fontconfig'), f'{cfg}/fontconfig')
        prefs = json.load(open(TEMPLATE)); s = prefs['settings']
        s.update(theme='midnight', follow_system=False, interface_text_size=size, project_pane=False)
        prefs['recent_repositories'] = [repo]; prefs['project_library'] = [{'project': {'path': repo}}]
        json.dump(prefs, open(f'{cfg}/gitturtle/preferences.json', 'w'), indent=2)
        env = dict(os.environ, XDG_CONFIG_HOME=cfg, XDG_DATA_HOME=f'{cfg}/data', XDG_STATE_HOME=f'{cfg}/state', XDG_CACHE_HOME=f'{cfg}/cache')
        env.pop('GITTURTLE_TRACE', None)
        if measured: env.update(NO_AT_BRIDGE='1', WAYLAND_DEBUG='1')
        else: env.pop('NO_AT_BRIDGE', None); env.pop('WAYLAND_DEBUG', None)
        os.makedirs(f'{TOP}/raw', exist_ok=True)
        self.log = open(f'{TOP}/raw/{label}.wl.log' if measured else f'{TOP}/raw/{label}.out.log', 'w')
        hypr('hl.dsp.focus({ workspace = "9" })')
        self.marks = {'label': label, 'binary': os.path.basename(binary), 'fixture': fixture, 'size': size, 'window': [w, h],
                      'loadavg_start': open('/proc/loadavg').read().split()[:3], 'events': []}
        self.proc = subprocess.Popen(['taskset', '-c', '4-11', binary, repo], env=env, stdout=self.log, stderr=subprocess.STDOUT,
                                     stdin=subprocess.DEVNULL, start_new_session=True)
        self.P = P = self.proc.pid; self.marks['pid'] = P
        atexit.register(lambda: self.proc.poll() is None and (self.proc.kill(), self.proc.wait()))
        for _ in range(100):
            if self.client(): break
            time.sleep(0.1)
        else: self.proc.kill(); sys.exit('no window')
        time.sleep(2)
        self.wsel = f'window = "pid:{P}"'; self.fillers = []
        if w >= 1000 and h >= 680:
            hypr(f'hl.dsp.window.float({{ action = "enable", {self.wsel} }})'); self.resize(w, h)
            subprocess.run(['hyprctl', 'dispatch', f'hl.dsp.window.center({{ {self.wsel} }})'], capture_output=True)
        else:  # below the app's 1000 x 680 minimum: tiled beside one foot filler and above another
            hypr(f'hl.dsp.window.float({{ action = "disable", {self.wsel} }})'); time.sleep(0.5)
            self.fillers.append(subprocess.Popen(['foot', '--app-id', 'qa-filler', 'sleep', '900'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)); time.sleep(1)
            hypr(f'hl.dsp.focus({{ {self.wsel} }})')
            self.fillers.append(subprocess.Popen(['foot', '--app-id', 'qa-filler', 'sleep', '900'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)); time.sleep(1.2)
            atexit.register(lambda: [f.kill() for f in self.fillers])
            self.fit(w, h)
        hypr(f'hl.dsp.window.set_prop({{ {self.wsel}, prop = "opaque", value = "1" }})'); time.sleep(1)
        hypr(f'hl.dsp.focus({{ {self.wsel} }})')
    def client(self):
        for c in json.loads(sh('hyprctl', 'clients', '-j')):
            if c['pid'] == self.P: return c
    def resize(self, w, h):
        if self.fillers: self.fit(w, h, settle=0)
        else: hypr(f'hl.dsp.window.resize({{ x = {w}, y = {h}, {self.wsel} }})')
    def fit(self, w, h, settle=1.0):
        for _ in range(6):
            _, _, cw, ch = self.geom()
            if (cw, ch) == (w, h): time.sleep(settle); return
            hypr(f'hl.dsp.window.resize({{ x = {w - cw}, y = {h - ch}, relative = true, {self.wsel} }})'); time.sleep(0.6)
        raise SystemExit(f'fit failed: {self.geom()}')
    def geom(self): c = self.client(); return (*c['at'], *c['size'])
    def vptr(self, *a): subprocess.run([VPTR, *map(str, a)], check=True, capture_output=True)
    def move(self, x, y): gx, gy, _, _ = self.geom(); self.vptr('move', gx + x, gy + y)
    def click(self, x, y, settle=0.7):
        self.move(x, y); time.sleep(settle); self.vptr('down', 'sleep', 40, 'up')
    def key(self, *a): subprocess.run(['wtype', *a], check=True); time.sleep(0.45)
    def sample(self, name, **extra):
        P = self.P
        with open(f'/proc/{P}/task/{P}/schedstat') as f: cpu_ns = int(f.read().split()[0])
        with open(f'/proc/{P}/task/{P}/stat') as f: cpu = int(f.read().rsplit(')', 1)[1].split()[36])
        with open(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq') as f: khz = int(f.read())
        self.marks['events'].append(dict(name=name, rt_us=time.clock_gettime_ns(time.CLOCK_REALTIME) // 1000, ui_cpu_ns=cpu_ns, cpu=cpu, khz=khz, **extra))
    def nodes(self, text='', all=True):
        return subprocess.run(['/usr/bin/python3', FIND, str(self.P), text] + (['--all'] if all else []), capture_output=True, text=True).stdout
    def node(self, text):
        out = self.nodes(text, all=False).split()
        return (int(out[0]) // 2, int(out[1]) // 2) if len(out) == 2 else None
    def shot(self, name): subprocess.run(['grim', '-g', '%d,%d %dx%d' % self.geom(), f'{TOP}/raw/{name}.png'])
    def finish(self):
        self.marks['loadavg_end'] = open('/proc/loadavg').read().split()[:3]
        os.kill(self.P, signal.SIGTERM)
        try: self.proc.wait(10)
        except subprocess.TimeoutExpired: self.proc.kill(); self.proc.wait()
        for f in self.fillers: f.kill(); f.wait()
        self.marks['exit'] = self.proc.returncode
        subprocess.run(['hyprctl', 'switchxkblayout', 'apple-mtp-keyboard', '0'], capture_output=True)
        json.dump(self.marks, open(f'{TOP}/raw/{self.label}.marks.json', 'w'), indent=1)
