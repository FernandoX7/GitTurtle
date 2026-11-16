#!/usr/bin/env python3
"""tables.py: the record's Markdown tables from raw/summary.json (summary.py)."""
import json, os
RAW = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'raw')
S = json.load(open(f'{RAW}/summary.json')); B, C = '5f7b078', '5e12885'
CELL = {'L13': '1480 × 980, 13 pt', 'L18': '1480 × 980, 18 pt', 'S13': '461 × 490, 13 pt', 'S18': '461 × 490, 18 pt'}
POOL = {'history': 'History', 'text': 'Text Compare', 'image': 'Image Compare'}
def ci(v, p=3): return f"{v[0]:+.{p}f} [{v[1][0]:+.{p}f}, {v[1][1]:+.{p}f}]" if v and v[1] else '—'
def q(d, k, p=3): return f"{d[k]:.{p}f}" if d.get('n') else '—'
print('### Per-frame draw by surface\n')
print('| Cell | Surface | Build | Paced draws n | Paced p50 / p95 / max (ms) | Launch p50s | With 1–2-tick draws p50 / p95 / p99 / max (ms) | Over one tick | Input-led draws n | Input-led p50 / p95 / p99 / max (ms) |')
print('| --- | --- | --- | ---: | --- | --- | --- | ---: | ---: | --- |')
for c in CELL:
    for p in POOL:
        r = S.get(f'{c}-{p}')
        if not r: continue
        for b in (B, C):
            x = r[b]; d, a, i = x['draw'], x['draw_amb'], x['draw_in']; lp = x['launch_p50']
            print(f"| {CELL[c] if (p == 'history' and b == B) else ''} | {POOL[p] if b == B else ''} | `{b}` | {d.get('n', 0):,} | {q(d,'p50')} / {q(d,'p95')} / {q(d,'max')} | {f'{lp[0]:.2f}–{lp[-1]:.2f}' if lp else '—'} | {q(a,'p50')} / {q(a,'p95')} / {q(a,'p99')} / {q(a,'max')} | {x['over_tick']} | {i.get('n', 0):,} | {q(i,'p50')} / {q(i,'p95')} / {q(i,'p99')} / {q(i,'max')} |")
print('\n### Candidate minus base, per-frame draw\n')
print('| Cell | Surface | Paced Δp50 (ms) | Δp95, with 1–2-tick draws | Δp99, with 1–2-tick draws | Input-led Δp50 | Input-led Δp95 | Input-led Δp99 |')
print('| --- | --- | --- | --- | --- | --- | --- | --- |')
for c in CELL:
    for p in POOL:
        r = S.get(f'{c}-{p}')
        if not r: continue
        d = r['delta']; print(f"| {CELL[c]} | {POOL[p]} | {ci(d['draw_p50'])} | {ci(d['draw_amb_p95'])} | {ci(d['draw_amb_p99'])} | {ci(d['draw_in_p50'])} | {ci(d['draw_in_p95'])} | {ci(d['draw_in_p99'])} |")
print('\n### Frames, draws and UI-thread CPU per interaction\n')
print('| Cell | Interaction | Build | n | Frames p50 / p95 (min–max) | UI CPU p50 / p95 / max (ms) | Input to first frame request p50 / p95 / p99 / max (ms) | Δ UI CPU p50 (ms) | Δ input to first request p50 (ms) |')
# Wheel-notch rows: one input-led frame per notch, so the input-to-request value is that frame's handler plus draw.
print('| --- | --- | --- | ---: | --- | --- | --- | --- | --- |')
NAMES = [('hwheel', 'History wheel notch'), ('hold', 'History held arrow, 1 s'), ('twheel', 'Text wheel notch'), ('tmode', 'Text mode (Split, Diff)'),
         ('zoom', 'Image zoom (100%, 200%)'), ('drag', 'Image pan drag at 200%'), ('iwheel', 'Image wheel notch at 200%'), ('mode', 'Image mode (Overlay, Side by side)'), ('drag-overlay', 'Image pan drag, Overlay')]
for c in CELL:
    for ph, name in NAMES:
        r = S.get(f'{c}-{ph}')
        if not r: continue
        for b in (B, C):
            x = r[b]; fr, cp, fq = x['frames'], x['cpu'], x['first_request']; dl = r['delta']
            print(f"| {CELL[c] if (b == B and ph == NAMES[0][0]) else ''} | {name if b == B else ''} | `{b}` | {x['n']} | {fr['p50']} / {fr['p95']} ({fr['min']}–{fr['max']}) | {q(cp,'p50',1)} / {q(cp,'p95',1)} / {q(cp,'max',1)} | {q(fq,'p50',2)} / {q(fq,'p95',2)} / {q(fq,'p99',2)} / {q(fq,'max',2)} | {ci(dl['cpu_p50'],1) if b == C else ''} | {ci(dl['first_request_p50'],2) if b == C else ''} |")
print('\n### Width steps\n')
print('| Cell | Surface | Step | Build | n | Frames (count of steps) | Final without further input (steps) | UI CPU p50 / p95 / max (ms) | Configure to first frame request p50 / max (ms) | Last frame after the command p50 / max (ms) | Δ UI CPU p50 (ms) |')
print('| --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |')
SURF = {'hresize': 'History', 'tresize': 'Text Compare', 'iresize': 'Image Compare'}
for c in CELL:
    for ph in SURF:
        for t0, t1 in ((('1480', '1000'), ('1000', '1480')) if c[0] == 'L' else (('461', '980'), ('980', '461'))):
            r = S.get(f'{c}-{ph}@{t1}')
            if not r: continue
            for b in (B, C):
                x = r[b]; cp, fq, lf = x['cpu'], x['first_request'], x['last_frame']; st = x['settle']
                settled = f"{st.get('0', 0)} of {x['n']}"
                print(f"| {CELL[c] if (b == B and ph == 'hresize' and t1 in ('1000', '980')) else ''} | {SURF[ph] if (b == B and t1 in ('1000', '980')) else ''} | {t0 + ' → ' + t1 if b == B else ''} | `{b}` | {x['n']} | {', '.join(f'{k} ({v})' for k, v in x['frames_hist'].items())} | {settled} | {q(cp,'p50',1)} / {q(cp,'p95',1)} / {q(cp,'max',1)} | {q(fq,'p50',1)} / {q(fq,'max',1)} | {q(lf,'p50',1)} / {q(lf,'max',1)} | {ci(r['delta']['cpu_p50'],1) if b == C else ''} |")
