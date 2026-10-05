#!/usr/bin/env python3
"""Summarize run.sh's JSON lines: per case and build, pooled steady-state p50/p95/max of the apply work
(and, for the candidate, of the fit alone) in µs, the spread of process medians, the p50 delta, and the
first call in fresh processes. Usage: analyze.py RESULTS.jsonl SUMMARY.json"""
import json, sys, statistics


def pct(values, p):
    v = sorted(values)
    return v[max(0, -(-len(v) * p // 100) - 1)] if v else None


def us(ns):
    return None if ns is None else round(ns / 1000, 2)


rows = [json.loads(l) for l in open(sys.argv[1])]
cases = list(dict.fromkeys(r["case"] for r in rows))
out = {}
for case in cases:
    cell = {}
    for build in ("base", "cand"):
        steady = [r for r in rows if r["case"] == case and r["build"] == build and r["mode"] == "steady"]
        first = [r["apply_ns"][0] for r in rows if r["case"] == case and r["build"] == build and r["mode"] == "first"]
        apply_all = [v for r in steady for v in r["apply_ns"]]
        fit_all = [v for r in steady for v in r["fit_ns"]]
        cell[build] = {
            "procs": len(steady), "n": len(apply_all),
            "apply_us": {"p50": us(pct(apply_all, 50)), "p95": us(pct(apply_all, 95)), "max": us(max(apply_all))},
            "apply_proc_p50_us": [us(statistics.median(r["apply_ns"])) for r in steady],
            "fit_us": {"p50": us(pct(fit_all, 50)), "p95": us(pct(fit_all, 95)), "max": us(max(fit_all))} if fit_all else None,
            "first_us": {"n": len(first), "p50": us(pct(first, 50)), "max": us(max(first))} if first else None,
        }
    cell["delta_apply_p50_us"] = round(cell["cand"]["apply_us"]["p50"] - cell["base"]["apply_us"]["p50"], 2)
    cell["delta_apply_p95_us"] = round(cell["cand"]["apply_us"]["p95"] - cell["base"]["apply_us"]["p95"], 2)
    if cell["base"]["first_us"] and cell["cand"]["first_us"]:
        cell["delta_first_p50_us"] = round(cell["cand"]["first_us"]["p50"] - cell["base"]["first_us"]["p50"], 2)
    out[case] = cell
    b, c = cell["base"], cell["cand"]
    print(f"| {case} | {b['apply_us']['p50']} / {b['apply_us']['p95']} / {b['apply_us']['max']} | "
          f"{c['apply_us']['p50']} / {c['apply_us']['p95']} / {c['apply_us']['max']} | {cell['delta_apply_p50_us']:+} | "
          f"{c['fit_us']['p50']} / {c['fit_us']['p95']} | "
          f"{min(b['apply_proc_p50_us'])}–{max(b['apply_proc_p50_us'])}; {min(c['apply_proc_p50_us'])}–{max(c['apply_proc_p50_us'])} | "
          f"{b['first_us']['p50'] if b['first_us'] else ''} / {c['first_us']['p50'] if c['first_us'] else ''} |")
json.dump(out, open(sys.argv[2], "w"), indent=1)
