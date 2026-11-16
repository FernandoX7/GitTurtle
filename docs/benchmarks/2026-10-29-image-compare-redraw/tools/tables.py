#!/usr/bin/env python3
"""tables.py: the record's Markdown tables from raw/summary.json and raw/windows.csv (resize steps
split by direction, with the same two-level bootstrap as summary.py)."""
import csv, json, os, random
RAW = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'raw')
S = json.load(open(f'{RAW}/summary.json')); B, C = '6660805', '81db2e3'
CELL = {'L13': '1480 × 980, 13 pt', 'L18': '1480 × 980, 18 pt', 'S18': '461 × 490, 18 pt'}
FX = {'png': 'PNG changed', 'add': 'PNG added', 'gif': 'GIF changed'}
def ci(v, p=3): return f"{v[0]:+.{p}f} [{v[1][0]:+.{p}f}, {v[1][1]:+.{p}f}]" if v[1] else '—'
def q(d, k, p=3): return f"{d[k]:.{p}f}" if d.get('n') else '—'
print('### Per-frame draw, every interaction pooled\n')
print('| Cell | Fixture | Build | Draws n | p50 / p95 / max (ms), paced | Launch p50s | p50 / p95 / p99 / max (ms), with 1–2-tick draws | Draws over one tick |')
print('| --- | --- | --- | ---: | --- | --- | --- | ---: |')
for c in CELL:
    for f in FX:
        r = S.get(f'{f}-{c}-all')
        if not r: continue
        for b in (B, C):
            x = r[b]; d, a = x['draw'], x['draw_amb']; lp = x['launch_p50']
            print(f"| {CELL[c] if (f == 'png' and b == B) else ''} | {FX[f] if b == B else ''} | `{b}` | {d['n']:,} | {q(d,'p50')} / {q(d,'p95')} / {q(d,'max')} | {lp[0]:.2f}–{lp[-1]:.2f} | {q(a,'p50')} / {q(a,'p95')} / {q(a,'p99')} / {q(a,'max')} | {x['over_tick']} |")
print('\n### Candidate minus base, per-frame draw\n')
print('| Cell | Fixture | Draw Δp50 (ms) | Δp95, with 1–2-tick draws (ms) | Δp99, with 1–2-tick draws (ms) |')
print('| --- | --- | --- | --- | --- |')
for c in CELL:
    for f in FX:
        r = S.get(f'{f}-{c}-all')
        if not r: continue
        d = r['delta']; print(f"| {CELL[c]} | {FX[f]} | {ci(d['draw_p50'])} | {ci(d['draw_amb_p95'])} | {ci(d['draw_amb_p99'])} |")
print('\n### Frames, draws and UI-thread CPU per interaction\n')
print('| Cell | Interaction | Build | n | Frames p50 / p95 (min–max) | Draws p50 (min–max) | UI CPU p50 / p95 / max (ms) | Input to first frame request p50 / p95 / max (ms) | Δ UI CPU p50 (ms) | Δ draw p50 (ms) |')
print('| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |')
NAMES = [('png', 'drag', 'Pan drag at 200%, PNG changed'), ('png', 'drag-overlay', 'Pan drag at 200%, Overlay'), ('png', 'wheel', 'Wheel notch at 200% (pans)'),
         ('png', 'zoom', 'Zoom choice'), ('png', 'mode', 'Mode change'), ('add', 'drag', 'Pan drag at 200%, PNG added'), ('gif', 'play', 'GIF playback, 4 s'), ('gif', 'pause', 'GIF pause')]
for c in CELL:
    for f, ph, name in NAMES:
        r = S.get(f'{f}-{c}-{ph}')
        if not r: continue
        for b in (B, C):
            x = r[b]; fr, dr, cp, fq = x['frames'], x['draws'], x['cpu'], x['first_request']
            dl = r['delta']
            print(f"| {CELL[c] if b == B else ''} | {name if b == B else ''} | `{b}` | {x['n']} | {fr['p50']} / {fr['p95']} ({fr['min']}–{fr['max']}) | {dr['p50']} ({dr['min']}–{dr['max']}) | {q(cp,'p50',1)} / {q(cp,'p95',1)} / {q(cp,'max',1)} | {q(fq,'p50',2)} / {q(fq,'p95',2)} / {q(fq,'max',2)} | {ci(dl['cpu_p50'],1) if b == C else ''} | {(ci(dl['draw_p50']) if x['draw'].get('n') else '—') if b == C else ''} |")
print('\n### Width steps\n')
W = [r for r in csv.DictReader(open(f'{RAW}/windows.csv')) if r['phase'] == 'resize']
def pct(v, qq): v = sorted(v); return v[min(len(v) - 1, max(0, int(round(qq * len(v) + 0.5)) - 1))]
rng = random.Random(1)
def boot(bl, cl):
    def one(ls): d = [rng.choice(ls) for _ in ls]; return pct([x for l in d for x in (rng.choice(l) for _ in l)], .5)
    ds = sorted(one(cl) - one(bl) for _ in range(4000)); return ds[100], ds[3899]
print('| Cell | Step | Build | n | Frames (count of steps) | UI CPU p50 / p95 / max (ms) | Configure to first frame request p50 / p95 / max (ms) | Last frame after the command p50 / max (ms) | Δ UI CPU p50 (ms) |')
print('| --- | --- | --- | ---: | --- | --- | --- | --- | --- |')
for c in CELL:
    for t0, t1 in ((('1480', '1000'), ('1000', '1480')) if c[0] == 'L' else (('700', '461'), ('461', '700'))):
        res = {}
        for b in (B, C):
            rows = [r for r in W if r['cell'] == c and r['target'] == t1 and r['build'] == b]
            hist = {}; [hist.__setitem__(r['frames'], hist.get(r['frames'], 0) + 1) for r in rows]
            cpu = [float(r['ui_cpu_ms']) for r in rows]; fq = [float(r['first_request_ms']) for r in rows]; last = [float(r['last_frame_ms']) for r in rows]
            per = {}; [per.setdefault(r['launch'], []).append(float(r['ui_cpu_ms'])) for r in rows]
            res[b] = (rows, hist, cpu, fq, last, list(per.values()))
        lo, hi = boot(res[B][5], res[C][5])
        for b in (B, C):
            rows, hist, cpu, fq, last, _ = res[b]
            print(f"| {CELL[c] if (b == B and t1 in ('1000', '461')) else ''} | {t0 + ' → ' + t1 if b == B else ''} | `{b}` | {len(rows)} | {', '.join(f'{k} ({v})' for k, v in sorted(hist.items()))} | {pct(cpu,.5):.1f} / {pct(cpu,.95):.1f} / {max(cpu):.1f} | {pct(fq,.5):.1f} / {pct(fq,.95):.1f} / {max(fq):.1f} | {pct(last,.5):.1f} / {max(last):.1f} | {f'{pct(res[C][2],.5) - pct(res[B][2],.5):+.1f} [{lo:+.1f}, {hi:+.1f}]' if b == C else ''} |")
