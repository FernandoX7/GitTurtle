#!/usr/bin/env python3
"""analyse.py LABEL... : parse raw/LABEL.wl.log (WAYLAND_DEBUG=1, wall-clock µs stamps) with
raw/LABEL.marks.json and append to raw/windows.csv (one row per interaction or idle window) and
raw/frames.csv.gz rows to raw/frames.csv (one row per frame drawn on the main surface in a window).
Boundaries as in docs/benchmarks/2026-10-29-changes-body-redraw/tools/analyse.py:
  main surface  the wl_surface first given to xdg_wm_base.get_xdg_surface; popups are not counted.
  frame         one wl_surface.attach on it (Mesa's present of one GPUI draw).
  gap_ms        trigger dispatch to the app's next wl_surface.frame request on the main surface.
                'present' (a done less than 0.5 ms before the request: GPUI presented again without
                drawing; no draw time), 'paced' (a main-surface wl_callback.done 0.5 ms to one
                8.33 ms tick before the request: the draw), 'ambiguous' (done one to two ticks before), 'woken' (none: no draw time),
                'input' (a wl_pointer button press), 'configure' (xdg_toplevel.configure).
  present_ms    frame request to the attach.
Window start t0: the app's own first wl_pointer.button press in the driver's window (zoom, mode,
play, pause, drag, drag-overlay), its first wl_pointer axis event (wheel), or the driver's start
sample (resize, idle). Window end: the driver's end sample. ui_cpu_ms: the main thread's schedstat
run time between the driver's two samples. motions: wl_pointer.motion events dispatched in it."""
import csv, json, os, re, sys
TOP = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); RAW = f'{TOP}/raw'; OUT = os.environ.get('ANALYSE_OUT', RAW)  # where windows.csv and frames.csv go
LINE = re.compile(r'^\[(\d\d):(\d\d):(\d\d)\.(\d{6})\]\s+(?:\{([^}]*)\}\s+)?(->\s+)?(?:discarded\s+)?([\w]+)#(\d+)\.(\w+)\((.*)\)\s*$')
def us(h, m, s, f): return ((int(h) * 60 + int(m)) * 60 + int(s)) * 1_000_000 + int(f)
def parse(path):
    main = None; cb_main = {}; ev = []
    for line in open(path, errors='replace'):
        m = LINE.match(line)
        if not m: continue
        t = us(*m.group(1, 2, 3, 4)); out, iface, oid, msg, args = bool(m.group(6)), m.group(7), int(m.group(8)), m.group(9), m.group(10)
        if out and iface == 'xdg_wm_base' and msg == 'get_xdg_surface' and main is None:
            main = int(re.search(r'wl_surface#(\d+)', args).group(1))
        if iface == 'wl_surface' and oid == main and out:
            if msg == 'frame': cb = int(re.search(r'wl_callback#(\d+)', args).group(1)); cb_main[cb] = True; ev.append((t, 'request', cb))
            elif msg == 'attach': ev.append((t, 'attach', None))
        elif iface == 'wl_callback' and not out and msg == 'done' and cb_main.pop(oid, False): ev.append((t, 'done', oid))
        elif iface == 'wl_pointer' and not out:
            if msg == 'button':
                a = [x.strip() for x in args.split(',')]
                if a[2] == '272' and a[3] == '1': ev.append((t, 'press', None))
            elif msg == 'motion': ev.append((t, 'motion', None))
            elif msg in ('axis', 'axis_discrete', 'axis_value120'): ev.append((t, 'axis', None))
        elif iface == 'wl_keyboard' and not out and msg == 'key':
            a = [x.strip() for x in args.split(',')]
            if a[3] == '1': ev.append((t, 'key', None))
        elif iface == 'xdg_toplevel' and not out and msg == 'configure': ev.append((t, 'configure', args))
    return main, ev
TICK = 1000 / 120  # ms, eDP-1 at 120 Hz
# A frame request this soon after the done had no Window::draw before it: GPUI presented without
# drawing (observed gaps < 0.05 ms; every draw took >= 1.5 ms).
NODRAW = 0.5
def frames_of(ev):
    out = []; trig = None; last_req = None; pend = None
    for t, k, a in ev:
        if k in ('done', 'press', 'configure', 'key'): trig = (t, k)
        elif k == 'request':
            if trig and (last_req is None or trig[0] > last_req):
                gap = (t - trig[0]) / 1000
                kind = {'press': 'input', 'key': 'input', 'configure': 'configure'}.get(trig[1]) or ('present' if gap < NODRAW else 'paced' if gap <= TICK else 'ambiguous' if gap <= 2 * TICK else 'woken')
                if kind == 'woken': gap = None
            else: kind, gap = 'woken', None
            pend = (t, kind, gap); last_req = t
        elif k == 'attach':
            out.append((t, pend[1], pend[2], (t - pend[0]) / 1000, pend[0]) if pend else (t, 'unpaired', None, None, None)); pend = None
    return out
WFIELDS = ['launch', 'build', 'fixture', 'cell', 'round', 'phase', 'index', 'target', 'ui_cpu_ms', 'frames', 'draws', 'present', 'paced', 'ambiguous', 'woken', 'input', 'configure', 'configures', 'presses', 'keys', 'motions', 'axes', 'settle_px', 'first_request_ms', 'last_frame_ms', 'cpu', 'khz_start', 'khz_end', 'window_ms']
FFIELDS = ['launch', 'build', 'fixture', 'cell', 'round', 'phase', 'index', 'frame', 't_ms', 'start', 'gap_ms', 'present_ms']
PRESSED = ('zoom', 'mode', 'tmode', 'drag', 'drag-overlay')
def main():
    wnew = not os.path.exists(f'{OUT}/windows.csv'); fnew = not os.path.exists(f'{OUT}/frames.csv')
    W = csv.DictWriter(open(f'{OUT}/windows.csv', 'a', newline=''), WFIELDS); F = csv.DictWriter(open(f'{OUT}/frames.csv', 'a', newline=''), FFIELDS)
    if wnew: W.writeheader()
    if fnew: F.writeheader()
    for label in sys.argv[1:]:
        marks = json.load(open(f'{RAW}/{label}.marks.json')); surface, ev = parse(f'{RAW}/{label}.wl.log'); fr = frames_of(ev)
        base = dict(launch=label, build=marks['build'], fixture=marks['fixture'], cell=marks['cell'], round=label.split('-')[0])
        E = marks['events']; day = lambda rt: rt % 86_400_000_000
        for i, s in enumerate(E):
            if not s['name'].endswith('-start'): continue
            e = next(x for x in E[i + 1:] if x['name'] == s['name'][:-6] + '-end' and x.get('index') == s.get('index'))
            phase = s['name'][:-6]; t0, t1 = day(s['rt_us']), day(e['rt_us'])
            inwin = lambda k: [t for t, kk, _ in ev if kk == k and t0 <= t < t1]
            if phase in PRESSED:
                p = inwin('press'); assert p, (label, phase, s.get('index')); t0 = p[0]
            elif phase.endswith('wheel'):
                a = inwin('axis'); assert a, (label, phase, s.get('index')); t0 = a[0]
            elif phase == 'hold':
                a = inwin('key'); assert a, (label, phase, s.get('index')); t0 = a[0]
            inside = [f for f in fr if t0 <= f[0] < t1]; req = [f[4] for f in inside if f[4] is not None]
            row = dict(base, phase=phase, index=s.get('index', ''), target=s.get('target', s.get('width', s.get('stroke', s.get('notch', '')))),
                       ui_cpu_ms=round((e['ui_cpu_ns'] - s['ui_cpu_ns']) / 1e6, 3), frames=len(inside),
                       **{c: sum(1 for f in inside if f[1] == c) for c in ('present', 'paced', 'ambiguous', 'woken', 'input', 'configure')},
                       draws=sum(1 for f in inside if f[1] != 'present'),
                       configures=len(inwin('configure')), presses=len(inwin('press')), keys=len(inwin('key')),
                       settle_px=next((x['diff_px'] for x in E if x['name'] == 'settle' and x['phase'] == phase and x['index'] == s.get('index')), ''), motions=len(inwin('motion')), axes=len(inwin('axis')),
                       first_request_ms=round((req[0] - t0) / 1000, 3) if req else '',
                       last_frame_ms=round((inside[-1][0] - t0) / 1000, 3) if inside else '',
                       cpu=s['cpu'], khz_start=s['khz'], khz_end=e['khz'], window_ms=round((t1 - t0) / 1000, 1))
            W.writerow(row)
            for j, f in enumerate(inside):
                F.writerow(dict(base, phase=phase, index=row['index'], frame=j, t_ms=round((f[0] - t0) / 1000, 3), start=f[1],
                                gap_ms='' if f[2] is None else round(f[2], 3), present_ms='' if f[3] is None else round(f[3], 3)))
        print(label, 'surface', surface, 'frames', len(fr), 'unpaired', sum(1 for f in fr if f[1] == 'unpaired'))
main()
