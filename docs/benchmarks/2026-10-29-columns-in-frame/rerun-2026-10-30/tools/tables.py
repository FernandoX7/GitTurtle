#!/usr/bin/env python3
"""tables.py (rerun 2026-10-30, width steps only): the record's width-step tables from raw/summary.json
(summary.py). Per cell, surface, direction and build: frames per step (p50, max and the count of steps
per frame count), steps final without further input (settle check 0), UI CPU per step p50 / p95 / max,
command to first frame request p50 / max (from the driver's sample, as the
record's 'Configure to first frame request' column, which spans the same), configure to first frame request
p50 / p95 (from raw/frames.csv(.gz): the first frame's gap after xdg_toplevel.configure), last frame after the command p50 / p95 / max, and the
candidate-minus-base UI CPU and last-frame p50 with their bootstrap intervals; then the paced draws
inside width-step windows per cell and surface."""
import csv, gzip, json, os
RAW = os.environ.get('SUMMARY_RAW') or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'raw')
S = json.load(open(f'{RAW}/summary.json')); B, C = os.environ.get('BASE', '5e12885'), os.environ.get('CAND', 'aeab555')
CELL = {'L13': '1480 × 980, 13 pt', 'L18': '1480 × 980, 18 pt', 'S13': '461 × 490, 13 pt', 'S18': '461 × 490, 18 pt'}
SURF = {'hresize': 'History', 'tresize': 'Text Compare', 'iresize': 'Image Compare'}
FR = os.path.join(RAW, 'frames.csv.gz'); FR = gzip.open(FR, 'rt') if os.path.exists(FR) else open(os.path.join(RAW, 'frames.csv'))
WN = {(r['launch'], r['phase'], r['index']): r['target'] for r in csv.DictReader(open(os.path.join(RAW, 'windows.csv')))}
CFG = {}
for f in csv.DictReader(FR):
    if f['frame'] == '0' and f['start'] == 'configure' and f['gap_ms']:
        CFG.setdefault((f['cell'], f['build'], f"{f['phase']}@{WN[(f['launch'], f['phase'], f['index'])]}"), []).append(float(f['gap_ms']))
def pct(v, q): v = sorted(v); return v[min(len(v) - 1, max(0, int(round(q * len(v) + 0.5)) - 1))]
def cfg(c, b, k): v = CFG.get((c, b, k), []); return f"{pct(v, .5):.1f} / {pct(v, .95):.1f}" if v else '—'
def ci(v, p=1): return f"{v[0]:+.{p}f} [{v[1][0]:+.{p}f}, {v[1][1]:+.{p}f}]" if v and v[1] else '—'
def q(d, k, p=1): return f"{d[k]:.{p}f}" if d.get('n') else '—'
print('### Width steps\n')
print('| Cell | Surface | Step | Build | n | Frames p50 / max | Frames (count of steps) | Final without further input | UI CPU p50 / p95 / max (ms) | Command to first frame request p50 / max (ms) | Configure to first frame request p50 / p95 (ms) | Last frame after the command p50 / p95 / max (ms) | Δ UI CPU p50 (ms) | Δ last frame p50 (ms) |')
print('| --- | --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |')
for c in CELL:
    for ph in SURF:
        for t0, t1 in ((('1480', '1000'), ('1000', '1480')) if c[0] == 'L' else (('461', '980'), ('980', '461'))):
            r = S.get(f'{c}-{ph}@{t1}')
            if not r: continue
            for b in (B, C):
                x = r[b]; cp, fq, lf, fr = x['cpu'], x['first_request'], x['last_frame'], x['frames']; st = x['settle']
                first = b == B and t1 in ('1000', '980')
                print(f"| {CELL[c] if (first and ph == 'hresize') else ''} | {SURF[ph] if first else ''} | {t0 + ' → ' + t1 if b == B else ''} | `{b}` | {x['n']} | {fr['p50']} / {fr['max']} | {', '.join(f'{k} ({v})' for k, v in x['frames_hist'].items())} | {st.get('0', 0)} of {x['n']} | {q(cp,'p50')} / {q(cp,'p95')} / {q(cp,'max')} | {q(fq,'p50')} / {q(fq,'max')} | {cfg(c, b, f'{ph}@{t1}')} | {q(lf,'p50')} / {q(lf,'p95')} / {q(lf,'max')} | {ci(r['delta']['cpu_p50']) if b == C else ''} | {ci(r['delta']['last_frame_p50']) if b == C else ''} |")
print('\n### Paced draws inside width-step windows\n')
print('| Cell | Surface | Build | Paced draws n | Paced p50 / p95 / max (ms) | With 1–2-tick draws n, p95 / max (ms) |')
print('| --- | --- | --- | ---: | --- | --- |')
for c in CELL:
    for ph in SURF:
        r = S.get(f'{c}-{ph}')
        if not r: continue
        for b in (B, C):
            x = r[b]; d, a = x['draw'], x['draw_amb']
            print(f"| {CELL[c] if (b == B and ph == 'hresize') else ''} | {SURF[ph] if b == B else ''} | `{b}` | {d.get('n', 0)} | {q(d,'p50',3)} / {q(d,'p95',3)} / {q(d,'max',3)} | {a.get('n', 0)}, {q(a,'p95',3)} / {q(a,'max',3)} |")
