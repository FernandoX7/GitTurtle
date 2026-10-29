#!/usr/bin/python3
"""summary.py: per cell (fixture, size) and build, from raw/windows.csv and raw/frames.csv, nearest-
rank percentiles of: frames per write and per resize step (and their start kinds); UI-thread CPU
per write and per resize step; press-to-first-frame-request; per-frame draw (paced frames' done
to frame request) in write windows and in resize windows; idle windows. Candidate-minus-base
deltas of draw p50/p95 and write CPU p50 with a two-level bootstrap (launches with replacement
within each build, then samples within each drawn launch; 4,000 resamples, seed 1).
Writes raw/summary.json and prints the tables."""
import csv, json, os, random, statistics, collections
RAW = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'raw')
BASE, CAND = 'ea58f87', '12b7043'
def pct(v, q):
    v = sorted(v); return v[min(len(v) - 1, max(0, int(round(q * len(v) + 0.5)) - 1))]
def stats(v): return dict(n=len(v), p50=pct(v, .5), p95=pct(v, .95), max=max(v), min=min(v)) if v else dict(n=0)
W = list(csv.DictReader(open(f'{RAW}/windows.csv'))); F = list(csv.DictReader(open(f'{RAW}/frames.csv')))
cells = sorted({(r['fixture'], int(r['size'])) for r in W}, key=lambda c: (c[0] != 'six', c[1]))
rng = random.Random(1)
def per_launch(rows, field):
    d = collections.OrderedDict()
    for r in rows:
        if r[field] != '': d.setdefault(r['launch'], []).append(float(r[field]))
    return list(d.values())
def boot(bl, cl, q):
    def one(ls):
        draw = [rng.choice(ls) for _ in ls]; return pct([x for l in draw for x in (rng.choice(l) for _ in l)], q)
    ds = sorted(one(cl) - one(bl) for _ in range(4000)); return [round(ds[100], 3), round(ds[3899], 3)]
out = {}
for fx, sz in cells:
    key = f'{fx}-{sz}'; res = out[key] = {}
    for b in (BASE, CAND):
        w = [r for r in W if r['fixture'] == fx and int(r['size']) == sz and r['build'] == b]
        f = [r for r in F if r['fixture'] == fx and int(r['size']) == sz and r['build'] == b]
        wr = [r for r in w if r['phase'] == 'write']; rs = [r for r in w if r['phase'] == 'resize']; idle = [r for r in w if r['phase'].startswith('idle')]
        fw = [r for r in f if r['phase'] == 'write']; fr = [r for r in f if r['phase'] == 'resize']
        res[b] = dict(
            launches=len({r['launch'] for r in w}),
            write_frames=stats([int(r['frames']) for r in wr]),
            write_frames_hist=dict(sorted(collections.Counter(int(r['frames']) for r in wr).items())),
            write_starts={k: stats([int(r[k]) for r in wr]) for k in ('paced', 'ambiguous', 'woken', 'input')},
            write_cpu_ms=stats([float(r['ui_cpu_ms']) for r in wr]),
            write_last_frame_ms=stats([float(r['last_frame_ms']) for r in wr if r['last_frame_ms']]),
            press_to_first_request_ms=stats([float(r['press_to_first_request_ms']) for r in wr if r['press_to_first_request_ms']]),
            write_draw_ms=stats([float(r['gap_ms']) for r in fw if r['start'] == 'paced']),
            write_draw_launch_p50=sorted(round(pct(l, .5), 3) for l in per_launch([r for r in fw if r['start'] == 'paced'], 'gap_ms')),
            write_ambiguous_gaps=sorted(float(r['gap_ms']) for r in fw if r['start'] == 'ambiguous'),
            resize_frames=stats([int(r['frames']) for r in rs]),
            resize_frames_hist=dict(sorted(collections.Counter(int(r['frames']) for r in rs).items())),
            resize_configures=dict(sorted(collections.Counter(int(r['configures']) for r in rs).items())),
            resize_paced=dict(sorted(collections.Counter(int(r['paced']) for r in rs).items())),
            resize_cpu_ms=stats([float(r['ui_cpu_ms']) for r in rs]),
            configure_to_first_request_ms=stats([float(r['gap_ms']) for r in fr if r['start'] == 'configure']),
            resize_draw_ms=stats([float(r['gap_ms']) for r in fr if r['start'] == 'paced']),
            idle_frames=sum(int(r['frames']) for r in idle), idle_cpu_ms=stats([float(r['ui_cpu_ms']) for r in idle]),
            khz=stats([int(r['khz_start']) for r in w] + [int(r['khz_end']) for r in w]),
            cpus=sorted({int(r['cpu']) for r in w}))
    bl = lambda b, ph: per_launch([r for r in F if r['fixture'] == fx and int(r['size']) == sz and r['build'] == b and r['phase'] == ph and r['start'] == 'paced'], 'gap_ms')
    wc = lambda b: per_launch([r for r in W if r['fixture'] == fx and int(r['size']) == sz and r['build'] == b and r['phase'] == 'write'], 'ui_cpu_ms')
    rc = lambda b: per_launch([r for r in W if r['fixture'] == fx and int(r['size']) == sz and r['build'] == b and r['phase'] == 'resize'], 'ui_cpu_ms')
    res['delta'] = {
        'write_draw_p50': [round(res[CAND]['write_draw_ms']['p50'] - res[BASE]['write_draw_ms']['p50'], 3), boot(bl(BASE, 'write'), bl(CAND, 'write'), .5)],
        'write_draw_p95': [round(res[CAND]['write_draw_ms']['p95'] - res[BASE]['write_draw_ms']['p95'], 3), boot(bl(BASE, 'write'), bl(CAND, 'write'), .95)],
        'write_cpu_p50': [round(res[CAND]['write_cpu_ms']['p50'] - res[BASE]['write_cpu_ms']['p50'], 3), boot(wc(BASE), wc(CAND), .5)],
        'resize_cpu_p50': [round(res[CAND]['resize_cpu_ms']['p50'] - res[BASE]['resize_cpu_ms']['p50'], 3), boot(rc(BASE), rc(CAND), .5)],
    }
json.dump(out, open(f'{RAW}/summary.json', 'w'), indent=1)
def s3(d, k='p50'): return '%.3f' % d[k]
for key, res in out.items():
    print(f'== {key}')
    for b in (BASE, CAND):
        r = res[b]; wf, rf = r['write_frames'], r['resize_frames']
        print(f" {b} launches {r['launches']} | writes n={wf['n']} frames p50 {wf['p50']} p95 {wf['p95']} min {wf['min']} max {wf['max']} hist {r['write_frames_hist']} "
              f"woken p50 {r['write_starts']['woken']['p50']} max {r['write_starts']['woken']['max']} amb max {r['write_starts']['ambiguous']['max']}")
        print(f"   write CPU ms p50 {s3(r['write_cpu_ms'])} p95 {s3(r['write_cpu_ms'], 'p95')} max {s3(r['write_cpu_ms'], 'max')} | press->1st req p50 {s3(r['press_to_first_request_ms'])} p95 {s3(r['press_to_first_request_ms'], 'p95')} max {s3(r['press_to_first_request_ms'], 'max')} | last frame max {s3(r['write_last_frame_ms'], 'max')}")
        d = r['write_draw_ms']; print(f"   draw (write, paced) n={d['n']} p50 {s3(d)} p95 {s3(d, 'p95')} max {s3(d, 'max')} launch p50s {r['write_draw_launch_p50'][0]}-{r['write_draw_launch_p50'][-1]} ambiguous {r['write_ambiguous_gaps']}")
        d = r['resize_draw_ms']; c = r['configure_to_first_request_ms']
        print(f"   resize n={rf['n']} frames hist {r['resize_frames_hist']} configures {r['resize_configures']} paced {r['resize_paced']} CPU p50 {s3(r['resize_cpu_ms'])} p95 {s3(r['resize_cpu_ms'], 'p95')} max {s3(r['resize_cpu_ms'], 'max')} | cfg->1st req p50 {s3(c)} p95 {s3(c, 'p95')} max {s3(c, 'max')} | draw n={d['n']} p50 {s3(d)} p95 {s3(d, 'p95')} max {s3(d, 'max')}")
        print(f"   idle frames {r['idle_frames']} idle CPU max {s3(r['idle_cpu_ms'], 'max')} | freq kHz p50 {r['khz']['p50']} min {r['khz']['min']} | cpus {r['cpus']}")
    for k, (d, ci) in res['delta'].items(): print(f'  delta {k} {d:+.3f} [{ci[0]:+.3f}, {ci[1]:+.3f}]')
