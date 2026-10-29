#!/usr/bin/env python3
"""drive.py BIN FIXTURE CELL LABEL [calibrate]: one launch of GitTurtle BIN on a fresh copy of
fixtures/pristine/FIXTURE (png|add|gif) in CELL (L13 = 1480 x 980 floating at 13 pt, L18 = the same
at 18 pt, S18 = 461 x 490 tiled at 18 pt; common.py has the launch). History opens with HEAD
selected; the driver clicks the changed file to enter Compare, then runs SEQUENCE (below).
Every click target is named by its accessible name. calibrate (accessibility bus on, no
WAYLAND_DEBUG) resolves each target with atspi_find.py just before clicking it and writes the
points, the drag geometry and screenshots to raw/cal/; a measured launch (NO_AT_BRIDGE=1,
WAYLAND_DEBUG=1) replays them. Windows ('<phase>-start' to '<phase>-end' samples of the UI thread's
schedstat, CPU and frequency) go to raw/LABEL.marks.json:
  zoom   choose a zoom (a button, or the Zoom menu and its item): pointer on the target, 1.2 s,
         sample, press+release (each click of a menu step), 1.5 s, sample.
  drag   at 200% (not in S18: the base draws no image there): press at one end of a horizontal stroke of A px through the image centre P,
         A/4 moves of 4 px about 8 ms apart, release, 1.0 s, sample. Alternating direction.
  wheel  (not in S18) pointer at P, sample, one wheel notch (alternating down/up), 1.0 s, sample.
  mode   choose Overlay (then two 'drag-overlay' strokes) and Side by side again, as zoom.
  play / pause  GIF: Play GIF then 4.0 s, Pause GIF then 1.5 s.
  resize width W0 -> W1 -> W0 ... (L: 1480 <-> 1000; S: 461 <-> 700), pointer parked on the status
         bar, sample, resize, 1.5 s, sample.
  idle   2.0 s with nothing done, after opening and at the end."""
import json, os, sys, time; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from common import *
from PIL import Image
binary, fixture, cell, label = sys.argv[1:5]; cal = len(sys.argv) > 5 and sys.argv[5] == 'calibrate'
size = 13 if cell == 'L13' else 18; W0, H = (1480, 980) if cell[0] == 'L' else (461, 490); W1 = 1000 if cell[0] == 'L' else 700
build = os.path.basename(binary).split('-')[1]; CAL = f'{TOP}/raw/cal/{build}-{fixture}-{cell}'
FILE = 'anim.gif · ' if fixture == 'gif' else 'photo.png · '
if fixture == 'gif':
    SEQ = [('open',), ('idle', 0)] + [('zoom', z) for z in (['100%', 'Fit'] * 2 if cell[0] == 'L' else [])] + [x for _ in range(3) for x in (('play',), ('pause',))] + [('resize',)] * 4 + [('idle', 1)]
    # not in S18: the base's GIF controls take the whole column there, leaving no zoom control or image
else:
    SEQ = [('open',), ('idle', 0)] + [('zoom', z) for z in (['100%', '50%', 'Fit', '200%'] if cell[0] == 'L' else ['100%', '50%', '200%', 'Fit']) * 2] + [('geometry',)] + [('drag',)] * 6 + [('wheel',)] * 10 \
        + [('mode', 'Overlay'), ('drag-overlay',), ('drag-overlay',), ('mode', 'Side by side')] + [('resize',)] * 6 + [('idle', 1)]
    if cell[0] == 'S':  # the base draws no image at 461 x 490 (its stage gets no room), so nothing to drag or scroll in both builds
        SEQ = [s for s in SEQ if s[0] not in ('geometry', 'drag', 'wheel', 'drag-overlay')]
L = Launch(binary, fixture, size, label, W0, H, measured=not cal)
pts = {} if cal else json.load(open(f'{CAL}.json'))
L.marks.update(cell=cell, build=build)
def park(): _, _, w, h = L.geom(); L.move(min(w, W0, W1) // 2, h - 6); time.sleep(0.2)
def targets(key, names):
    """Points of the clicks of step `key`: each of `names` is a list of alternatives (a direct
    target, or a menu opener followed by its item)."""
    if not cal: return [tuple(p) for p in pts[key]]
    out = []
    for alt in names:
        p = L.node(alt[0])
        if p: out.append(p); continue
        m = L.node(alt[1]); assert m, (key, alt); L.click(*m); time.sleep(1.0)
        i = L.node(alt[2]); assert i, (key, alt); out += [m, i]
        L.key('-k', 'Escape'); time.sleep(0.8)
    pts[key] = out; return out
def clicks(ps, between=0.6):
    for j, p in enumerate(ps):
        if j: L.move(*p); time.sleep(between)
        L.vptr('down', 'sleep', 40, 'up')
def compare_open(ref, now):
    a = Image.open(ref).convert('L').resize((96, 64)); b = Image.open(now).convert('L').resize((96, 64))
    return sum(abs(x - y) for x, y in zip(a.getdata(), b.getdata())) / (96 * 64)
def geometry():
    """The drag centre P and stroke length A: the saturated pixels' bounds in the left image side (the After side for add)."""
    L.shot(f'cal/{build}-{fixture}-{cell}-200'); im = Image.open(f'{TOP}/raw/cal/{build}-{fixture}-{cell}-200.png').convert('HSV')
    w, h = im.size; s = im.getchannel('S').load(); xs, ys = [], []
    # the preview column only: right of the navigation rail (and its app icon), left of the
    # inspector; the left half of it unless Before is absent (add), where the After side is used
    lo, hi = (0.15, 0.39 if fixture != 'add' else 0.6) if cell[0] == 'S' else (0.04, 0.40 if fixture != 'add' else 0.76)
    for y in range(0, h - 120, 4):
        for x in range(int(w * lo), int(w * hi), 4):
            if s[x, y] > 140: xs.append(x); ys.append(y)
    x0, x1, y0, y1 = min(xs) // 2, max(xs) // 2, min(ys) // 2, max(ys) // 2
    pts['P'] = [(x0 + x1) // 2, (y0 + y1) // 2]; pts['A'] = min(160, int(0.6 * (x1 - x0)) // 4 * 4); pts['region'] = [x0, y0, x1, y1]
kz = {}
def nk(k): kz[k] = kz.get(k, -1) + 1; return kz[k]
os.makedirs(f'{TOP}/raw/cal', exist_ok=True)
L.key('-k', 'Escape'); L.key('-k', 'Escape'); park(); time.sleep(1.0)
for step in SEQ:
    kind = step[0]
    if kind == 'open':
        p = targets('open', [[FILE]])[0]
        for attempt in range(3):
            L.click(*p); time.sleep(2.5); park(); time.sleep(0.5)
            shot = f'{label}-open' if not cal else f'cal/{build}-{fixture}-{cell}-open'; L.shot(shot)
            if cal: break
            d = compare_open(f'{CAL}-open.png', f'{TOP}/raw/{shot}.png'); L.marks.setdefault('open_diff', []).append(round(d, 2))
            if d < 12: break
        else: L.finish(); sys.exit(f'{label}: Compare did not open {L.marks["open_diff"]}')
    elif kind == 'idle':
        i = step[1]; L.sample('idle-start', index=i); time.sleep(2.0); L.sample('idle-end', index=i)
    elif kind in ('zoom', 'mode', 'play', 'pause'):
        if kind == 'zoom': z = step[1]; names = [['=' + z, 'Zoom: ', '=' + z]]
        elif kind == 'mode': z = step[1]; names = [['=' + z, 'Comparison mode: ', '=' + z]]
        else: z = kind; names = [['=Play GIF' if kind == 'play' else '=Pause GIF']]
        i = nk(kind); ps = targets(f'{kind}-{i}', names)
        L.move(*ps[0]); time.sleep(1.2); L.sample(f'{kind}-start', index=i, target=z); clicks(ps)
        time.sleep(4.0 if kind == 'play' else 1.5); L.sample(f'{kind}-end', index=i, target=z)
    elif kind == 'geometry':
        if cal: geometry()
    elif kind in ('drag', 'drag-overlay'):
        i = nk(kind); (px_, py), A = pts['P'], pts['A']; d = 1 if i % 2 == 0 else -1
        L.move(px_ - d * A // 2, py); time.sleep(1.0); gx, gy, _, _ = L.geom()
        args = ['down', 'sleep', 30]
        for k in range(1, A // 4 + 1): args += ['move', gx + px_ - d * A // 2 + d * 4 * k, gy + py, 'sleep', 7]
        L.sample(f'{kind}-start', index=i, stroke=A * d); L.vptr(*args, 'up'); time.sleep(1.0); L.sample(f'{kind}-end', index=i)
    elif kind == 'wheel':
        i = nk(kind); L.move(*pts['P']); time.sleep(0.8); L.sample('wheel-start', index=i, notch=1 if i % 2 == 0 else -1)
        subprocess.run([VPTR, 'stdin'], input=f'w {1 if i % 2 == 0 else -1}\nq\n', text=True, check=True, capture_output=True)
        time.sleep(1.0); L.sample('wheel-end', index=i)
    elif kind == 'resize':
        i = nk(kind); w = W1 if i % 2 == 0 else W0; park(); time.sleep(0.8)
        L.sample('resize-start', index=i, width=w); L.resize(w, H); time.sleep(1.5); L.sample('resize-end', index=i, width=w, size=list(L.geom()[2:]))
        if cal: L.shot(f'cal/{build}-{fixture}-{cell}-w{w}')
if cal: json.dump(pts, open(f'{CAL}.json', 'w')); print(label, 'points', json.dumps(pts))
L.finish(); print(label, 'done exit', L.marks['exit'], 'open_diff', L.marks.get('open_diff'), 'load', L.marks['loadavg_start'], '->', L.marks['loadavg_end'])
