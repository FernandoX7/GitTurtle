#!/usr/bin/env python3
"""summary.py: per cell, phase (or pool) and build, from raw/windows.csv and raw/frames.csv(.gz), as
bench-image-redraw-2026-10-29/tools/summary.py: nearest-rank percentiles of frames, draws and UI-thread
CPU per window, input to first frame request, and the per-frame draw ('paced' gaps <= one 8.33 ms tick;
and with 'ambiguous' 1-2-tick gaps). Pools: history (hwheel, hold, hresize), text (twheel, tmode,
tresize), image (zoom, drag, iwheel, mode, drag-overlay, iresize), all (every phase but idle).
Candidate-minus-base deltas: two-level bootstrap (launches with replacement within each build, then
samples within each drawn launch; 4,000 resamples, seed 1; 95% interval). Width steps are also split
by direction (phase@target). Writes raw/summary.json; prints the tables."""
import csv, gzip, json, os, random, collections
RAW = os.environ.get('SUMMARY_RAW') or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'raw')
BASE, CAND = '5f7b078', '5e12885'
def pct(v, q): v = sorted(v); return v[min(len(v) - 1, max(0, int(round(q * len(v) + 0.5)) - 1))]
def st(v): return dict(n=len(v), p50=pct(v, .5), p95=pct(v, .95), p99=pct(v, .99), max=max(v), min=min(v)) if v else dict(n=0)
W = list(csv.DictReader(open(f'{RAW}/windows.csv')))
F = list(csv.DictReader(gzip.open(f'{RAW}/frames.csv.gz', 'rt') if os.path.exists(f'{RAW}/frames.csv.gz') else open(f'{RAW}/frames.csv')))
rng = random.Random(1)
WG, FG = {}, {}
RES = ('hresize', 'tresize', 'iresize')
for r in W:
    WG.setdefault((r['cell'], r['build'], r['phase']), []).append(r)
    if r['phase'] in RES: WG.setdefault((r['cell'], r['build'], f"{r['phase']}@{r['target']}"), []).append(r)
for r in F:
    FG.setdefault((r['cell'], r['build'], r['phase']), []).append(r)
def per_launch(rows, f):
    d = collections.OrderedDict()
    for r in rows:
        if r[f] != '': d.setdefault(r['launch'], []).append(float(r[f]))
    return list(d.values())
def boot(bl, cl, q):
    if not bl or not cl: return None
    def one(ls):
        draw = [rng.choice(ls) for _ in ls]; return pct([x for l in draw for x in (rng.choice(l) for _ in l)], q)
    ds = sorted(one(cl) - one(bl) for _ in range(4000)); return [round(ds[100], 3), round(ds[3899], 3)]
POOLS = {'history': ['hwheel', 'hold', 'hresize'], 'text': ['twheel', 'tmode', 'tresize'],
         'image': ['zoom', 'drag', 'iwheel', 'mode', 'drag-overlay', 'iresize']}
POOLS['all'] = POOLS['history'] + POOLS['text'] + POOLS['image']
CELLS = ['L13', 'L18', 'S13', 'S18']
PH = sorted({k[2] for k in WG}) + list(POOLS)
out = {}
for c in CELLS:
    for ph in PH:
        phs = POOLS.get(ph, [ph])
        GW = lambda b: [r for p in phs for r in WG.get((c, b, p), [])]
        GF = lambda b: ([r for p in phs for r in FG.get((c, b, p), [])] if '@' not in ph else
                        [r for r in FG.get((c, b, ph.split('@')[0]), []) if (r['launch'], r['index']) in {(w['launch'], w['index']) for w in GW(b)}])
        res = {}
        for b in (BASE, CAND):
            w = GW(b); f = GF(b)
            if not w: continue
            paced = [float(r['gap_ms']) for r in f if r['start'] == 'paced']; amb = [float(r['gap_ms']) for r in f if r['start'] == 'ambiguous']
            res[b] = dict(launches=len({r['launch'] for r in w}), n=len(w),
                frames=st([int(r['frames']) for r in w]), draws=st([int(r['draws']) for r in w]), presents=st([int(r['present']) for r in w]),
                frames_hist=dict(sorted(collections.Counter(int(r['frames']) for r in w).items())),
                draws_hist=dict(sorted(collections.Counter(int(r['draws']) for r in w).items())),
                woken=st([int(r['woken']) for r in w]), configures=dict(sorted(collections.Counter(int(r['configures']) for r in w).items())),
                settle=dict(sorted(collections.Counter(('0' if r['settle_px'] == '0' else 'changed' if r['settle_px'] else 'none') for r in w).items())),
                settle_px=sorted(int(r['settle_px']) for r in w if r['settle_px'] not in ('', '0')),
                cpu=st([float(r['ui_cpu_ms']) for r in w]), first_request=st([float(r['first_request_ms']) for r in w if r['first_request_ms']]),
                last_frame=st([float(r['last_frame_ms']) for r in w if r['last_frame_ms']]),
                draw_in=st([float(r['gap_ms']) for r in f if r['start'] == 'input' and r['gap_ms']]),
                draw=st(paced), draw_amb=st(paced + amb), over_tick=len(amb),
                launch_p50=sorted(round(pct(l, .5), 3) for l in per_launch([r for r in f if r['start'] == 'paced'], 'gap_ms')),
                khz=st([int(r['khz_start']) for r in w]), below3ghz=sum(1 for r in w if int(r['khz_start']) < 3_000_000))
        if len(res) < 2: continue
        fl = lambda b, starts: per_launch([r for r in GF(b) if r['start'] in starts], 'gap_ms')
        wl = lambda b, k: per_launch(GW(b), k)
        if ph != 'idle':
            res['delta'] = {
                'draw_p50': [round(res[CAND]['draw'].get('p50', 0) - res[BASE]['draw'].get('p50', 0), 3), boot(fl(BASE, ('paced',)), fl(CAND, ('paced',)), .5)],
                'draw_p95': [round(res[CAND]['draw'].get('p95', 0) - res[BASE]['draw'].get('p95', 0), 3), boot(fl(BASE, ('paced',)), fl(CAND, ('paced',)), .95)],
                'draw_amb_p95': [round(res[CAND]['draw_amb'].get('p95', 0) - res[BASE]['draw_amb'].get('p95', 0), 3), boot(fl(BASE, ('paced', 'ambiguous')), fl(CAND, ('paced', 'ambiguous')), .95)],
                'draw_amb_p99': [round(res[CAND]['draw_amb'].get('p99', 0) - res[BASE]['draw_amb'].get('p99', 0), 3), boot(fl(BASE, ('paced', 'ambiguous')), fl(CAND, ('paced', 'ambiguous')), .99)],
                'draw_in_p50': [round(res[CAND]['draw_in'].get('p50', 0) - res[BASE]['draw_in'].get('p50', 0), 3), boot(fl(BASE, ('input',)), fl(CAND, ('input',)), .5)],
                'draw_in_p95': [round(res[CAND]['draw_in'].get('p95', 0) - res[BASE]['draw_in'].get('p95', 0), 3), boot(fl(BASE, ('input',)), fl(CAND, ('input',)), .95)],
                'draw_in_p99': [round(res[CAND]['draw_in'].get('p99', 0) - res[BASE]['draw_in'].get('p99', 0), 3), boot(fl(BASE, ('input',)), fl(CAND, ('input',)), .99)],
                'first_request_p50': [round(res[CAND]['first_request'].get('p50', 0) - res[BASE]['first_request'].get('p50', 0), 3), boot(wl(BASE, 'first_request_ms'), wl(CAND, 'first_request_ms'), .5)],
                'first_request_p95': [round(res[CAND]['first_request'].get('p95', 0) - res[BASE]['first_request'].get('p95', 0), 3), boot(wl(BASE, 'first_request_ms'), wl(CAND, 'first_request_ms'), .95)],
                'first_request_p99': [round(res[CAND]['first_request'].get('p99', 0) - res[BASE]['first_request'].get('p99', 0), 3), boot(wl(BASE, 'first_request_ms'), wl(CAND, 'first_request_ms'), .99)],
                'frames_p50': [res[CAND]['frames']['p50'] - res[BASE]['frames']['p50'], boot(wl(BASE, 'frames'), wl(CAND, 'frames'), .5)],
                'draws_p50': [res[CAND]['draws']['p50'] - res[BASE]['draws']['p50'], boot(wl(BASE, 'draws'), wl(CAND, 'draws'), .5)],
                'cpu_p50': [round(res[CAND]['cpu']['p50'] - res[BASE]['cpu']['p50'], 3), boot(wl(BASE, 'ui_cpu_ms'), wl(CAND, 'ui_cpu_ms'), .5)],
                'cpu_p95': [round(res[CAND]['cpu']['p95'] - res[BASE]['cpu']['p95'], 3), boot(wl(BASE, 'ui_cpu_ms'), wl(CAND, 'ui_cpu_ms'), .95)],
            }
        out[f'{c}-{ph}'] = res
json.dump(out, open(f'{RAW}/summary.json', 'w'), indent=1)
f3 = lambda d, k: '%.3f' % d[k] if d.get('n') else '-'
f1 = lambda d, k: '%.1f' % d[k] if d.get('n') else '-'
for key, res in out.items():
    print(f'== {key}')
    for b in (BASE, CAND):
        r = res[b]; fr, cp, d, da, fq = r['frames'], r['cpu'], r['draw'], r['draw_amb'], r['first_request']
        print(f"  {b} L{r['launches']} n={r['n']} frames {r['frames_hist']} draws p50 {r['draws']['p50']} ({r['draws']['min']}-{r['draws']['max']}) woken p50 {r['woken']['p50']} cfg {r['configures']} settle {r['settle']} {r['settle_px'][:6]}"
              f" | CPU p50 {f1(cp,'p50')} p95 {f1(cp,'p95')} max {f1(cp,'max')} | 1st req p50 {f3(fq,'p50')} p95 {f3(fq,'p95')} | last {f1(r['last_frame'],'p50')}/{f1(r['last_frame'],'max')}")
        di = r['draw_in']; print(f"     input draw n={di.get('n')} p50 {f3(di,'p50')} p95 {f3(di,'p95')} p99 {f3(di,'p99')} max {f3(di,'max')}")
        print(f"     draw n={d.get('n')} p50 {f3(d,'p50')} p95 {f3(d,'p95')} p99 {f3(d,'p99')} max {f3(d,'max')} launch p50s {r['launch_p50'][:1]}-{r['launch_p50'][-1:]} | +amb p95 {f3(da,'p95')} p99 {f3(da,'p99')} max {f3(da,'max')} over-tick {r['over_tick']} | <3GHz {r['below3ghz']}/{r['n']}")
    for k, v in res.get('delta', {}).items(): print(f'    delta {k} {v[0]:+} {v[1]}')
