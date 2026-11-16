#!/usr/bin/env python3
"""drive.py BIN CELL LABEL: one measured launch (NO_AT_BRIDGE=1, WAYLAND_DEBUG=1; common.py, copied
from bench-image-redraw-2026-10-29) of GitTurtle BIN on a fresh copy of fixtures/pristine/cols in
CELL: L13/L18 = 1480 x 980 floating at 13/18 pt, S13/S18 = 461 x 490 tiled at 13/18 pt. No
accessibility bus: Compare is entered with the keyboard (Enter twice from History; Escape twice
back) and the text mode, zoom and image mode buttons are found by OCR (tesseract on a screenshot,
pinned to CPUs 0-3, outside every measured window); L cells only, the S cells' toolbars are menus.
Each window is '<phase>-start' .. '<phase>-end' samples of the UI thread's schedstat, CPU and kHz.
History, HEAD selected, list focused:
  idle      2.0 s.
  hwheel    pointer on a History row; 10 unrecorded warm-up notches, then 60 notches (30 down, 30
            up), one per 150 ms window (as history_scroll_wayland.py).
  hold      Down held 1.0 s (wtype -P/-p; the app repeats at the seat's 40/s after 250 ms), 1.0 s
            more; then Up the same; twice. Then Home (unrecorded).
  hresize   four width steps W0 -> W1 -> W0 ... (L: 1480 <-> 1000; S: 461 <-> 980), pointer parked
            on the status bar, sample, resize, 1.5 s, sample. After each: the settle check (below).
Text and image Compare are not run in S13 (History only there).
Text Compare (HEAD's long.txt, 60 hunks):
  twheel    pointer on the diff; 20 notches (10 down, 10 up), 0.5 s windows.
  tmode     (L) Split, then Diff, each found by OCR just before (pointer parked): pointer on it 1.2 s, sample, click, 1.5 s, sample.
  tresize   four width steps with the settle check.
Image Compare (HEAD~1's photo.png, 1600 x 1000 changed):
  zoom      (L) 100%, then 200%, as tmode.
  drag      (L) four pan drags at 200% through the left side (stroke as bench-image-redraw).
  iwheel    (L) six wheel notches at 200%, 1.0 s windows.
  mode      (L) Overlay, one drag-overlay, Side by side.
  iresize   four width steps with the settle check.
  idle      2.0 s.
Settle check (outside the windows): screenshot A 1.5 s after the step, then the pointer visits the
Menu button (hover, an input that redraws) for 0.6 s and returns to the status bar for 0.8 s,
screenshot B; 'settle' events record how many device pixels differ by more than 24 grey levels,
masking the parked pointer (the compositor draws it into the screenshots). Pairs kept for round 1."""
import json, os, sys, time, subprocess; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); from common import *
from PIL import Image, ImageChops
binary, cell, label = sys.argv[1:4]
size = 13 if cell.endswith('13') else 18; wide = cell[0] == 'L'
W0, H = (1480, 980) if wide else (461, 490); W1 = 1000 if wide else 980
build = os.path.basename(binary).split('-')[1]
L = Launch(binary, 'cols', size, label, W0, H, measured=True)
L.marks.update(cell=cell, build=build, points={})
def park(): _, _, w, h = L.geom(); L.move(min(w, W0, W1) // 2, h - 6); time.sleep(0.2)
def ocr(name, ymax):
    """{word: (x, y)} window-relative logical centres of OCR words above ymax (logical)."""
    p = f'{TOP}/raw/{label}-{name}.png'; L.shot(f'{label}-{name}')
    out = subprocess.run(['taskset', '-c', '0-3', 'tesseract', p, '-', '--psm', '11', 'tsv'], capture_output=True, text=True).stdout
    words = {}
    for line in out.splitlines()[1:]:
        f = line.split('\t')
        if len(f) < 12 or not f[11].strip() or float(f[10]) < 40: continue
        x, y = (int(f[6]) + int(f[8]) / 2) / 2, (int(f[7]) + int(f[9]) / 2) / 2
        if y < ymax: words.setdefault(f[11].strip(), (int(x), int(y)))
    L.marks['points'][name] = words; return words
def cursor():
    gx, gy, _, _ = L.geom(); x, y = (int(float(v)) for v in sh('hyprctl', 'cursorpos').split(','))
    return 2 * (x - gx), 2 * (y - gy)
def settle(phase, i):
    a = f'{TOP}/raw/{label}-{phase}{i}-a.png'; b = f'{TOP}/raw/{label}-{phase}{i}-b.png'
    ca = cursor(); L.shot(f'{label}-{phase}{i}-a'); L.move(30, 14); time.sleep(0.6); park(); time.sleep(0.8); cb = cursor(); L.shot(f'{label}-{phase}{i}-b')
    ia, ib = Image.open(a).convert('L'), Image.open(b).convert('L')
    d = ImageChops.difference(ia, ib).point(lambda v: 255 if v > 24 else 0) if ia.size == ib.size else None
    if d is not None:
        for px_, py_ in (ca, cb): d.paste(0, (px_ - 24, py_ - 24, px_ + 48, py_ + 56))  # the pointer, which the compositor draws into both
    n = sum(d.histogram()[255:]) if d is not None else -1
    L.marks['events'].append(dict(name='settle', phase=phase, index=i, diff_px=n, size=list(ia.size)))
    if n == 0 or not label.startswith(('r1-', 'trial')): os.remove(a); os.remove(b)
def resize(phase, steps):
    for i in range(steps):
        w = W1 if i % 2 == 0 else W0; park(); time.sleep(0.8)
        L.sample(f'{phase}-start', index=i, width=w); L.resize(w, H); time.sleep(1.5); L.sample(f'{phase}-end', index=i, width=w, size=list(L.geom()[2:]))
        settle(phase, i)
def click_window(phase, i, word, target, ymax, after=1.5):
    park(); time.sleep(0.4); p = ocr(f'{phase}{i}', ymax)[word]; os.remove(f'{TOP}/raw/{label}-{phase}{i}.png')
    L.move(*p); time.sleep(1.2); L.sample(f'{phase}-start', index=i, target=target); L.vptr('down', 'sleep', 40, 'up')
    time.sleep(after); L.sample(f'{phase}-end', index=i, target=target)
def idle(i): L.sample('idle-start', index=i); time.sleep(2.0); L.sample('idle-end', index=i)
def key(k): L.key('-k', k)
vp = subprocess.Popen([VPTR, 'stdin'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def notch(n):
    vp.stdin.write(f'w {n}\n'); vp.stdin.flush(); assert vp.stdout.readline().strip() == 'ok'
def wheel(phase, steps, period, record=True):
    for i, n in enumerate(steps):
        t = time.monotonic() + period
        if record: L.sample(f'{phase}-start', index=i, notch=n)
        notch(n)
        while time.monotonic() < t: time.sleep(0.002)
        if record: L.sample(f'{phase}-end', index=i)
key('Escape'); key('Escape'); park(); time.sleep(1.0)
# History
idle(0)
L.move(*((600, 500) if wide else (140, 262))); time.sleep(0.8)
wheel('hwheel', [1] * 5 + [-1] * 5, 0.15, record=False); time.sleep(0.5)
wheel('hwheel', [1] * 30 + [-1] * 30, 0.15)
park(); time.sleep(0.8)
for i, k in enumerate(['Down', 'Up', 'Down', 'Up']):
    L.sample('hold-start', index=i, target=k); subprocess.run(['wtype', '-P', k, '-s', '1000', '-p', k], check=True)
    time.sleep(1.0); L.sample('hold-end', index=i, target=k)
key('Home'); time.sleep(1.5)
resize('hresize', 4)
if cell == 'S13':  # History only at 461 x 490, 13 pt
    idle(1); vp.stdin.write('q\n'); vp.stdin.close(); vp.wait(); L.finish(); print(label, 'done exit', L.marks['exit'], 'settle', [e['diff_px'] for e in L.marks['events'] if e['name'] == 'settle']); sys.exit(0)
# Text Compare
key('Return'); key('Return'); time.sleep(2.5); park(); L.shot(f'{label}-text')
L.move(*((600, 600) if wide else (220, 350))); time.sleep(0.8)
wheel('twheel', [1] * 10 + [-1] * 10, 0.5)
if wide:
    for i, t in enumerate(['Split', 'Diff']): click_window('tmode', i, t, t, 360)
resize('tresize', 4)
key('Escape'); key('Escape'); time.sleep(1.0); key('Down'); time.sleep(1.0); key('Return'); key('Return'); time.sleep(2.5); park(); time.sleep(0.5)
# Image Compare
if wide:
    for i, t in enumerate(['100%', '200%']): click_window('zoom', i, t, t, 360)
    park(); time.sleep(0.5); L.shot(f'{label}-200')
    im = Image.open(f'{TOP}/raw/{label}-200.png').convert('HSV'); iw, ih = im.size; s = im.getchannel('S').load(); xs, ys = [], []
    for y in range(0, ih - 120, 4):
        for x in range(int(iw * 0.04), int(iw * 0.40), 4):
            if s[x, y] > 140: xs.append(x); ys.append(y)
    x0, x1, y0, y1 = min(xs) // 2, max(xs) // 2, min(ys) // 2, max(ys) // 2
    P = ((x0 + x1) // 2, (y0 + y1) // 2); A = min(160, int(0.6 * (x1 - x0)) // 4 * 4); L.marks['points']['drag'] = [P, A, [x0, y0, x1, y1]]
    def drag(phase, i):
        d = 1 if i % 2 == 0 else -1; L.move(P[0] - d * A // 2, P[1]); time.sleep(1.0); gx, gy, _, _ = L.geom()
        args = ['down', 'sleep', 30]
        for k in range(1, A // 4 + 1): args += ['move', gx + P[0] - d * A // 2 + d * 4 * k, gy + P[1], 'sleep', 7]
        L.sample(f'{phase}-start', index=i, stroke=A * d); L.vptr(*args, 'up'); time.sleep(1.0); L.sample(f'{phase}-end', index=i)
    for i in range(4): drag('drag', i)
    L.move(*P); time.sleep(0.8)
    wheel('iwheel', [1, -1] * 3, 1.0)
    click_window('mode', 0, 'Overlay', 'Overlay', 360); drag('drag-overlay', 0)
    click_window('mode', 1, 'Side', 'Side by side', 360)
resize('iresize', 4)
idle(1)
vp.stdin.write('q\n'); vp.stdin.close(); vp.wait()
L.finish(); print(label, 'done exit', L.marks['exit'], 'settle', [e['diff_px'] for e in L.marks['events'] if e['name'] == 'settle'], 'load', L.marks['loadavg_start'], '->', L.marks['loadavg_end'])
