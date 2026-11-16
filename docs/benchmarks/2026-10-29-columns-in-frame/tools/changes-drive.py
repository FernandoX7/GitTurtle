#!/usr/bin/python3
"""drive.py BIN FIXTURE SIZE LABEL [calibrate]: one isolated launch of GitTurtle BIN on a fresh copy
of fixtures/pristine/FIXTURE (six|many) at interface text SIZE pt, floating 1480 x 980 on
workspace 9, pinned with taskset -c 4-11, WAYLAND_DEBUG=1 to raw/LABEL.wl.log, NO_AT_BRIDGE=1
(except calibrate), empty absolute XDG homes under cfg/LABEL with fontconfig linked.

Measured launch: Changes (Ctrl+2), then WARM Stage/Unstage pairs unrecorded, WRITES recorded
writes alternating Stage and Unstage of alpha.txt (six) or item-000.txt (many) at points.json's points, each: pointer
moved to the control, 1.2 s settle, UI-thread schedstat and realtime sampled, press+release,
3.0 s window, sampled again; then a 3 s idle window; then RESIZES height steps 980->940->980...
(width 1480), each 1.2 s, pointer parked; then a 3 s idle window. Every action's realtime and
the UI (main) thread's CPU, its last CPU and that CPU's frequency go to raw/LABEL.marks.json.
calibrate: AT-SPI on, prints the Stage/Unstage controls' window-relative logical centres."""
import json, os, signal, subprocess, sys, time
HERE = os.path.dirname(os.path.abspath(__file__)); TOP = os.path.dirname(HERE)
# The native-QA helpers this driver uses live outside the repository; point
# GITTURTLE_QA_TOOLS at that directory.
QA_TOOLS = os.environ.get('GITTURTLE_QA_TOOLS', '.local/item8-2026-10-27/tools')
VPTR = os.path.join(QA_TOOLS, 'vptr/vptr')
FIND = os.path.join(QA_TOOLS, 'atspi_find.py')
TEMPLATE = os.path.join(QA_TOOLS, 'prefs-template.json')
binary, fixture, size, label = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
calibrate = len(sys.argv) > 5 and sys.argv[5] == 'calibrate'
WARM, WRITES, RESIZES = int(os.environ.get('WARM', 2)), int(os.environ.get('WRITES', 20)), int(os.environ.get('RESIZES', 10))
pts = json.load(open(f'{HERE}/points.json')) if not calibrate else {}
CELL = f'{fixture}-{size}'
TOGGLE = 'alpha.txt' if fixture == 'six' else 'item-000.txt'
def sh(*a, **k): return subprocess.run(a, check=True, capture_output=True, text=True, **k).stdout
def hypr(cmd): sh('hyprctl', 'dispatch', cmd)
# fixture
run = f'{TOP}/fixtures/run'; repo = f'{run}/{fixture}'; os.makedirs(run, exist_ok=True)
subprocess.run(['rm', '-rf', repo], check=True); subprocess.run(['cp', '-a', f'{TOP}/fixtures/pristine/{fixture}', repo], check=True)
# app data
cfg = f'{TOP}/cfg/{label}'; subprocess.run(['rm', '-rf', cfg], check=True)
for d in ('gitturtle', 'data', 'state', 'cache'): os.makedirs(f'{cfg}/{d}')
os.symlink(os.path.expanduser('~/.config/fontconfig'), f'{cfg}/fontconfig')
prefs = json.load(open(TEMPLATE)); s = prefs['settings']
s.update(theme='midnight', follow_system=False, interface_text_size=size, project_pane=False)
prefs['recent_repositories'] = [repo]; prefs['project_library'] = [{'project': {'path': repo}}]
json.dump(prefs, open(f'{cfg}/gitturtle/preferences.json', 'w'), indent=2)
env = dict(os.environ, XDG_CONFIG_HOME=cfg, XDG_DATA_HOME=f'{cfg}/data', XDG_STATE_HOME=f'{cfg}/state', XDG_CACHE_HOME=f'{cfg}/cache')
env.pop('GITTURTLE_TRACE', None)
if calibrate: env.pop('NO_AT_BRIDGE', None)
else: env.update(NO_AT_BRIDGE='1', WAYLAND_DEBUG='1')
os.makedirs(f'{TOP}/raw', exist_ok=True)
log = open(f'{TOP}/raw/{label}.wl.log', 'w')
hypr('hl.dsp.focus({ workspace = "9" })')
marks = {'label': label, 'binary': binary, 'fixture': fixture, 'size': size, 'loadavg_start': open('/proc/loadavg').read().split()[:3], 'events': []}
proc = subprocess.Popen(['taskset', '-c', '4-11', binary, repo], env=env, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True)
P = proc.pid; marks['pid'] = P
import atexit
atexit.register(lambda: proc.poll() is None and (proc.kill(), proc.wait()))
def client():
    for c in json.loads(sh('hyprctl', 'clients', '-j')):
        if c['pid'] == P: return c
for _ in range(100):
    if client(): break
    time.sleep(0.1)
else: proc.kill(); sys.exit('no window')
time.sleep(2)
w = f'window = "pid:{P}"'
hypr(f'hl.dsp.window.float({{ action = "enable", {w} }})'); hypr(f'hl.dsp.window.resize({{ x = 1480, y = 980, {w} }})')
subprocess.run(['hyprctl', 'dispatch', f'hl.dsp.window.center({{ {w} }})'], capture_output=True)
hypr(f'hl.dsp.window.set_prop({{ {w}, prop = "opaque", value = "1" }})'); time.sleep(1)
def geom(): c = client(); return (*c['at'], *c['size'])
def vptr(*a): subprocess.run([VPTR, *map(str, a)], check=True, capture_output=True)
def move(x, y): gx, gy, _, _ = geom(); vptr('move', gx + x, gy + y)
PARK = (740, 960)  # the status bar's middle, window-relative logical px
move(*PARK); time.sleep(0.2)
hypr(f'hl.dsp.focus({{ {w} }})')
def key(*a): subprocess.run(['wtype', *a], check=True); time.sleep(0.45)
key('-k', 'Escape'); key('-k', 'Escape'); key('-M', 'ctrl', '2', '-m', 'ctrl'); time.sleep(2.5)
STAT = f'/proc/{P}/task/{P}/schedstat'
def sample(name, **extra):
    with open(STAT) as f: cpu_ns = int(f.read().split()[0])
    with open(f'/proc/{P}/task/{P}/stat') as f: cpu = int(f.read().rsplit(')', 1)[1].split()[36])
    with open(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq') as f: khz = int(f.read())
    marks['events'].append(dict(name=name, rt_us=time.clock_gettime_ns(time.CLOCK_REALTIME) // 1000, ui_cpu_ns=cpu_ns, cpu=cpu, khz=khz, **extra))
def press(x, y): move(x, y); time.sleep(0.7); gx, gy, _, _ = geom(); return gx, gy
if calibrate:
    def node(t):
        out = subprocess.run(['/usr/bin/python3', FIND, str(P), t], capture_output=True, text=True).stdout.split()
        return (int(out[0]) // 2, int(out[1]) // 2) if len(out) == 2 else None
    found = {}
    subprocess.run(['grim', '-g', '%d,%d %dx%d' % geom(), f'{TOP}/raw/{label}-start.png'])
    open(f'{TOP}/raw/{label}-nodes.txt', 'w').write(subprocess.run(['/usr/bin/python3', FIND, str(P), '', '--all'], capture_output=True, text=True).stdout)
    st = node('Stage ' + TOGGLE); found['stage'] = st; print('Stage', TOGGLE, st, 'window', geom())
    move(*st); time.sleep(0.7); vptr('down', 'sleep', 40, 'up'); time.sleep(2.5)
    un = node('Unstage ' + TOGGLE); found['unstage'] = un; print('Unstage', TOGGLE, un)
    move(*un); time.sleep(0.7); vptr('down', 'sleep', 40, 'up'); time.sleep(2.5)
    print('Stage again', node('Stage ' + TOGGLE))
    subprocess.run(['grim', '-g', '%d,%d %dx%d' % geom(), f'{TOP}/raw/{label}.png'])
    json.dump(found, open(f'{TOP}/raw/{label}.points.json', 'w'))
else:
    stage, unstage = pts[CELL]['stage'], pts[CELL]['unstage']
    for i in range(WARM * 2):
        move(*(stage if i % 2 == 0 else unstage)); time.sleep(0.7); vptr('down', 'sleep', 40, 'up'); time.sleep(2.0)
    for k in range(WRITES):
        kind = 'stage' if k % 2 == 0 else 'unstage'
        move(*(stage if kind == 'stage' else unstage)); time.sleep(1.2)
        sample('write-start', kind=kind, index=k); vptr('down', 'sleep', 40, 'up')
        time.sleep(3.0); sample('write-end', kind=kind, index=k)
    move(*PARK); time.sleep(1.0)
    sample('idle1-start'); time.sleep(3.0); sample('idle1-end')
    for k in range(RESIZES):
        h = 940 if k % 2 == 0 else 980
        sample('resize-start', height=h, index=k); hypr(f'hl.dsp.window.resize({{ x = 1480, y = {h}, {w} }})')
        time.sleep(1.2); sample('resize-end', height=h, index=k)
    sample('idle2-start'); time.sleep(3.0); sample('idle2-end')
marks['loadavg_end'] = open('/proc/loadavg').read().split()[:3]
os.kill(P, signal.SIGTERM)
try: proc.wait(10)
except subprocess.TimeoutExpired: proc.kill(); proc.wait()
marks['exit'] = proc.returncode
marks['staged_after'] = subprocess.run(['git', '-C', repo, 'diff', '--cached', '--name-only'], env=dict(os.environ, GIT_OPTIONAL_LOCKS='0'), capture_output=True, text=True).stdout.split()
subprocess.run(['hyprctl', 'switchxkblayout', 'apple-mtp-keyboard', '0'], capture_output=True)
json.dump(marks, open(f'{TOP}/raw/{label}.marks.json', 'w'), indent=1)
print(label, 'done, exit', proc.returncode, 'staged after', marks['staged_after'], 'load', marks['loadavg_start'], '->', marks['loadavg_end'])
