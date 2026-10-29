#!/usr/bin/python3
"""analyse.py LABEL... : parse raw/LABEL.wl.log (WAYLAND_DEBUG=1, UTC wall-clock µs stamps) with
raw/LABEL.marks.json and append to raw/windows.csv (one row per write, resize or idle window) and
raw/frames.csv (one row per frame drawn on the main surface inside a window).

Main surface: the wl_surface given to xdg_wm_base.get_xdg_surface first. A frame is one
`wl_surface.attach` on it (Mesa's present). Per frame:
  start, gap_ms  see frames_of. For 'paced' frames gap_ms is the draw: `wl_callback.done`
             dispatch to GPUI's `wl_surface.frame` request, which the Wayland platform sends in
             `draw` after `Window::draw` returned and before the renderer runs. For 'input' it is
             the press event's dispatch through the handler and the draw to that request.
  present_ms that frame request to the attach (renderer encode, submit and Mesa present).
Write windows run from the app's own wl_pointer.button press event to the driver's write-end
sample; resize windows from the driver's resize-start sample to its resize-end sample; idle
windows between their two samples. ui_cpu_ms is the main thread's schedstat run time between
the driver's two samples of that window."""
import csv, json, os, re, sys
TOP = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); RAW = f'{TOP}/raw'
LINE = re.compile(r'^\[(\d\d):(\d\d):(\d\d)\.(\d{6})\]\s+(?:\{([^}]*)\}\s+)?(->\s+)?(?:discarded\s+)?([\w]+)#(\d+)\.(\w+)\((.*)\)\s*$')
def us(h, m, s, f): return ((int(h) * 60 + int(m)) * 60 + int(s)) * 1_000_000 + int(f)
def parse(path):
    main = None; cb_main = {}; ev = []
    for line in open(path, errors='replace'):
        m = LINE.match(line)
        if not m: continue
        t = us(*m.group(1, 2, 3, 4)); queue, out, iface, oid, msg, args = m.group(5), bool(m.group(6)), m.group(7), int(m.group(8)), m.group(9), m.group(10)
        if out and iface == 'xdg_wm_base' and msg == 'get_xdg_surface' and main is None:
            main = int(re.search(r'wl_surface#(\d+)', args).group(1))
        if iface == 'wl_surface' and oid == main and out:
            if msg == 'frame': cb = int(re.search(r'wl_callback#(\d+)', args).group(1)); cb_main[cb] = True; ev.append((t, 'request', cb))
            elif msg == 'attach': ev.append((t, 'attach', None))
        elif iface == 'wl_callback' and not out and msg == 'done' and cb_main.pop(oid, False): ev.append((t, 'done', oid))
        elif iface == 'wl_pointer' and not out and msg == 'button':
            a = [x.strip() for x in args.split(',')]
            if a[2] == '272' and a[3] == '1': ev.append((t, 'press', None))
        elif iface == 'xdg_toplevel' and not out and msg == 'configure': ev.append((t, 'configure', args))
    return main, ev
TICK = 1000 / 120  # ms, eDP-1 at 120 Hz
def frames_of(ev):
    """Per main-surface attach: (attach time, start, gap_ms, present_ms, request time). start is the
    latest trigger since the previous frame request: 'paced' (a main-surface wl_callback.done, gap
    <= one tick), 'ambiguous' (done, one to two ticks), 'woken' (done more than two ticks before,
    or no trigger: GPUI parked and was woken by a task), 'input' (wl_pointer press) or 'configure'
    (xdg_toplevel.configure). gap_ms is trigger dispatch to the frame request (None for woken)."""
    out = []; trig = None; last_req = None; pend = None
    for t, k, a in ev:
        if k in ('done', 'press', 'configure'): trig = (t, k)
        elif k == 'request':
            if trig and (last_req is None or trig[0] > last_req):
                gap = (t - trig[0]) / 1000
                kind = {'press': 'input', 'configure': 'configure'}.get(trig[1]) or ('paced' if gap <= TICK else 'ambiguous' if gap <= 2 * TICK else 'woken')
                if kind == 'woken': gap = None
            else: kind, gap = 'woken', None
            pend = (t, kind, gap); last_req = t
        elif k == 'attach':
            if pend: out.append((t, pend[1], pend[2], (t - pend[0]) / 1000, pend[0]))
            else: out.append((t, 'unpaired', None, None, None))
            pend = None
    return out
WFIELDS = ['launch', 'build', 'fixture', 'size', 'round', 'phase', 'index', 'kind', 'ui_cpu_ms', 'frames', 'paced', 'ambiguous', 'woken', 'input', 'configure', 'configures', 'first_frame_ms', 'last_frame_ms', 'press_to_first_request_ms', 'cpu', 'khz_start', 'khz_end', 'window_ms']
FFIELDS = ['launch', 'build', 'fixture', 'size', 'round', 'phase', 'index', 'kind', 'frame', 't_ms', 'start', 'gap_ms', 'present_ms']
def main():
    wnew = not os.path.exists(f'{RAW}/windows.csv'); fnew = not os.path.exists(f'{RAW}/frames.csv')
    wf = open(f'{RAW}/windows.csv', 'a', newline=''); ff = open(f'{RAW}/frames.csv', 'a', newline='')
    W = csv.DictWriter(wf, WFIELDS); F = csv.DictWriter(ff, FFIELDS)
    if wnew: W.writeheader()
    if fnew: F.writeheader()
    for label in sys.argv[1:]:
        marks = json.load(open(f'{RAW}/{label}.marks.json')); surface, ev = parse(f'{RAW}/{label}.wl.log')
        fr = frames_of(ev); presses = [t for t, k, _ in ev if k == 'press']
        build = os.path.basename(marks['binary']).split('-')[1]; rnd = label.split('-')[0]
        base = dict(launch=label, build=build, fixture=marks['fixture'], size=marks['size'], round=rnd)
        E = marks['events']; day = lambda rt: rt % 86_400_000_000
        pairs = []
        for i, e in enumerate(E):
            if e['name'].endswith('-start'):
                end = next(x for x in E[i + 1:] if x['name'] == e['name'][:-6] + '-end' and x.get('index') == e.get('index'))
                pairs.append((e, end))
        for s, e in pairs:
            phase = s['name'][:-6]; t0, t1 = day(s['rt_us']), day(e['rt_us'])
            if phase == 'write':
                p = [t for t in presses if t0 <= t < t1]
                assert len(p) == 1, (label, s, p); t0 = p[0]
            inside = [f for f in fr if t0 <= f[0] < t1]
            req = [f[4] for f in inside if f[4] is not None]
            row = dict(base, phase=phase, index=s.get('index', ''), kind=s.get('kind', s.get('height', '')),
                       ui_cpu_ms=round((e['ui_cpu_ns'] - s['ui_cpu_ns']) / 1e6, 3), frames=len(inside),
                       **{c: sum(1 for f in inside if f[1] == c) for c in ('paced', 'ambiguous', 'woken', 'input', 'configure')},
                       configures=sum(1 for t, k, _ in ev if k == 'configure' and t0 <= t < t1),
                       first_frame_ms=round((inside[0][0] - t0) / 1000, 3) if inside else '',
                       last_frame_ms=round((inside[-1][0] - t0) / 1000, 3) if inside else '',
                       press_to_first_request_ms=round((req[0] - t0) / 1000, 3) if (phase == 'write' and req) else '',
                       cpu=s['cpu'], khz_start=s['khz'], khz_end=e['khz'], window_ms=round((t1 - t0) / 1000, 1))
            W.writerow(row)
            for j, f in enumerate(inside):
                F.writerow(dict(base, phase=phase, index=row['index'], kind=row['kind'], frame=j, t_ms=round((f[0] - t0) / 1000, 3), start=f[1],
                                gap_ms='' if f[2] is None else round(f[2], 3), present_ms='' if f[3] is None else round(f[3], 3)))
        print(label, 'surface', surface, 'frames', len(fr), 'presses', len(presses))
main()
