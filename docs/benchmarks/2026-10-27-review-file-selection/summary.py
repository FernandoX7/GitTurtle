#!/usr/bin/env python3
"""summary.py RESULTS_DIR: nearest-rank p50, p95 and max per build and series, per round and pooled."""
import json, math, pathlib, sys
d = pathlib.Path(sys.argv[1])
def pct(v, q):
    v = sorted(v); return v[max(0, math.ceil(q * len(v)) - 1)]
pooled = {}
for f in sorted(d.glob('*.json')):
    build, rnd = f.stem.rsplit('-', 1)
    data = json.load(open(f))
    for s in data['series']:
        v = s['milliseconds']
        pooled.setdefault((s['name'], build), []).extend(v)
        print(f"{s['name']:18} {build:13} round {rnd}: n {len(v)} p50 {pct(v,.5):.3f} p95 {pct(v,.95):.3f} max {max(v):.3f}")
print()
for (name, build), v in sorted(pooled.items()):
    print(f"{name:18} {build:13} pooled: n {len(v)} p50 {pct(v,.5):.3f} p95 {pct(v,.95):.3f} max {max(v):.3f}")
