#!/usr/bin/env python3
"""regeometry.py: recompute the drag centre P, stroke A and region of the L cells from the
calibration's 200% screenshots (raw/cal/<build>-<fixture>-<cell>-200.png), with the search bounded
to the stage (y 30-85% of the window) and to the Before side (png: x 4-40%) or the After side (add,
whose Before is absent: x 42-76%). Writes them into both builds' calibration files."""
import json, os
from PIL import Image
TOP = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
for fx, (lo, hi) in (('png', (0.04, 0.40)), ('add', (0.42, 0.76))):
    for c in ('L13', 'L18'):
        geo = []
        for b in ('6660805', '81db2e3'):
            im = Image.open(f'{TOP}/raw/cal/{b}-{fx}-{c}-200.png').convert('HSV'); w, h = im.size; s = im.getchannel('S').load()
            pts = [(x, y) for y in range(int(h * .30), int(h * .85), 4) for x in range(int(w * lo), int(w * hi), 4) if s[x, y] > 140]
            xs, ys = [p[0] for p in pts], [p[1] for p in pts]; x0, x1, y0, y1 = min(xs) // 2, max(xs) // 2, min(ys) // 2, max(ys) // 2
            geo.append(dict(P=[(x0 + x1) // 2, (y0 + y1) // 2], A=min(160, int(0.6 * (x1 - x0)) // 4 * 4), region=[x0, y0, x1, y1]))
        print(fx, c, geo)
        for b in ('6660805', '81db2e3'):
            f = f'{TOP}/raw/cal/{b}-{fx}-{c}.json'; d = json.load(open(f)); d.update(geo[1] if b == '81db2e3' else geo[0]); json.dump(d, open(f, 'w'))
