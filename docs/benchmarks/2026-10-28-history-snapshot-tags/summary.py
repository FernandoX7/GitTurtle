import json, sys, glob, os, math
R = sys.argv[1]
def pct(v, p): v = sorted(v); return v[max(0, math.ceil(p / 100 * len(v)) - 1)]
for fixture in ("history-tags", "history-tags-packed", "history"):
    for series in ("load_ms", "refresh_ms"):
        print(f"{fixture} {series}")
        for build in ("base", "candidate"):
            pooled, rounds = [], []
            for f in sorted(glob.glob(f"{R}/{fixture}-{build}-*.json")):
                d = json.load(open(f)); pooled += d[series]
                rounds.append(f"{pct(d[series], 50):.2f}")
            print(f"  {build:9} n={len(pooled)} p50={pct(pooled,50):.2f} p95={pct(pooled,95):.2f} max={max(pooled):.2f} rounds p50 {' '.join(rounds)} commits={d['commits']} branches={d['branches']}")
