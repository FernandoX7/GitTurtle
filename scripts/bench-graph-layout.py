#!/usr/bin/env python3
"""Measure pure graph layout and row cloning, excluding native presentation.

Extracts the graph model/layout from the chosen source and compiles a temporary
standalone Rust harness with rustc -O. No dependencies, repository writes, or
native app launch are required. The old layout signature is supported for an
explicit baseline revision. Current layout includes an acquire cancellation
checkpoint for every row, matching the worker's successful checkpoint path.
Row-clone samples reproduce the owned geometry clones passed to 60 visible
paint callbacks. They exclude element construction, tessellation, and drawing.

Example:
  python3 scripts/bench-graph-layout.py --baseline-ref <revision> --output result.json
"""

import argparse
import hashlib
import json
import platform
import statistics
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GRAPH_PATH = "crates/app/src/graph.rs"


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def benchmark(stage, source, warmups, samples, directory):
    end = "/// A shared horizontal lane viewport" if "/// A shared horizontal lane viewport" in source else "/// One coordinate system"
    engine = source[source.index("#[derive(Clone, Debug, PartialEq, Eq)]"):source.index(end)]
    invocation = "layout(black_box(&commits))"
    if "pub fn layout<E>" in engine:
        invocation = (
            "layout(black_box(&commits), || "
            "if black_box(&latest).load(Ordering::Acquire) == 1 "
            "{ Ok(()) } else { Err(()) }).unwrap()"
        )
    harness = r"""
#![allow(unused_imports, unused_variables)]
use std::collections::{HashMap, HashSet};
use std::{hint::black_box, time::Instant, sync::{Arc, atomic::{AtomicU64, Ordering}}};
pub struct Commit { oid: String, parents: Vec<String> }
""" + engine + r"""
fn main() {
    let latest = AtomicU64::new(1);
    let linear: Vec<_> = (0..20000).map(|i| Commit {
        oid: format!("commit-{i}"), parents: vec![format!("commit-{}", i+1)]
    }).collect();
    let mut wide = vec![Commit {
        oid: "merge".into(), parents: (0..20).map(|i| format!("outside-{i}")).collect()
    }];
    wide.extend((0..8000).map(|i| Commit {
        oid: format!("commit-{i}"), parents: vec![format!("commit-{}", i+1)]
    }));
    for (name, commits) in [("linear-20000", linear), ("wide-21-lanes-8001", wide)] {
        for _ in 0..WARMUPS { black_box(INVOCATION); }
        let mut samples = Vec::new();
        for _ in 0..SAMPLES {
            let start = Instant::now();
            black_box(INVOCATION);
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
        println!("{name}: {samples:?}");
        let rows = INVOCATION;
        let clone_visible = || {
            for row in rows.iter().skip(1).take(60) {
                black_box(row.clone());
            }
        };
        for _ in 0..WARMUPS {
            for _ in 0..200 { clone_visible(); }
        }
        let mut samples = Vec::new();
        for _ in 0..SAMPLES {
            let start = Instant::now();
            for _ in 0..200 { clone_visible(); }
            // A batch stabilizes the timer for this small amount of work.
            samples.push(start.elapsed().as_secs_f64() * 1000. / 200.);
        }
        println!("clone-visible-60-{name}: {samples:?}");
    }
}
"""
    harness = harness.replace("WARMUPS", str(warmups)).replace("SAMPLES", str(samples))
    harness = harness.replace("INVOCATION", invocation)
    rust = directory / f"graph-{stage}.rs"
    executable = directory / f"graph-{stage}"
    rust.write_text(harness)
    subprocess.run(["rustc", "-O", "--edition", "2024", str(rust), "-o", str(executable)], check=True)
    result = command(str(executable))
    cases = []
    for line in result.splitlines():
        fixture, data = line.split(": ", 1)
        values = json.loads(data)
        ordered = sorted(values)
        cases.append({
            "stage": stage,
            "fixture": fixture,
            "median_ms": statistics.median(values),
            "p95_ms": ordered[(len(ordered) * 95 + 99) // 100 - 1],
            "max_ms": ordered[-1],
            "raw_ms": values,
            "harness_sha256": hashlib.sha256(harness.encode()).hexdigest(),
            "graph_source_sha256": hashlib.sha256(source.encode()).hexdigest(),
        })
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-ref", help="Explicit local Git revision for the old layout")
    parser.add_argument("--output", type=Path, help="Write JSON here; defaults to standard output")
    parser.add_argument("--warmups", type=int, default=20)
    parser.add_argument("--samples", type=int, default=100)
    args = parser.parse_args()
    if args.warmups < 0 or args.samples < 1:
        parser.error("warmups must be nonnegative and samples must be positive")
    result = {
        "measured_at_utc": datetime.now(timezone.utc).isoformat(),
        "hardware": command("sysctl", "-n", "machdep.cpu.brand_string")
            if platform.system() == "Darwin" else platform.machine(),
        "os": platform.platform(),
        "rustc": command("rustc", "--version"),
        "working_tree_base_revision": command("git", "rev-parse", "HEAD"),
        "baseline_revision": command("git", "rev-parse", "--verify", args.baseline_ref)
            if args.baseline_ref else None,
        "benchmark": "Pure layout and row cloning extracted from graph.rs, compiled "
            "with rustc -O; inputs reused in memory, output destruction included. "
            "No Git, GPUI, preflight, OS presentation, or end-to-end navigation "
            "measurement. Current layout includes an AtomicU64 acquire checkpoint "
            "per row. Clone cases measure 60 row clones and drops per simulated "
            "frame, averaged over 200 frames per sample; this excludes element "
            "construction, tessellation, and drawing.",
        "warmups_per_case": args.warmups,
        "samples_per_case": args.samples,
        "other_load": "Uncontrolled; close competing work before comparing runs.",
        "cases": [],
    }
    with tempfile.TemporaryDirectory(prefix="gitturtle-graph-bench-") as temporary:
        directory = Path(temporary)
        if args.baseline_ref:
            baseline = command("git", "show", f"{result['baseline_revision']}:{GRAPH_PATH}")
            result["cases"].extend(benchmark("before", baseline, args.warmups, args.samples, directory))
        result["cases"].extend(benchmark("after", (ROOT / GRAPH_PATH).read_text(), args.warmups, args.samples, directory))
    encoded = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    else:
        print(encoded, end="")


if __name__ == "__main__":
    main()
