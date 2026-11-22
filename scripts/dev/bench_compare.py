#!/usr/bin/env python3
"""Deterministic release benchmark comparison driver for GitTurtle.

Runs alternating baseline vs candidate benchmark rounds, optionally acquiring
`build_slot.py quiet-on`/`quiet-off` and pinning to P-cores via `taskset`,
records binary SHA-256 digests, and checks statistical significance against
the measured noise floor (median + IQR separation).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
BUILD_SLOT = ROOT / "scripts" / "dev" / "build_slot.py"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def summarize(samples_ms: list[float]) -> dict[str, float]:
    if not samples_ms:
        raise ValueError("at least one sample is required")
    ordered = sorted(samples_ms)
    median = statistics.median(ordered)
    if len(ordered) >= 4:
        q = statistics.quantiles(ordered, n=4, method="inclusive")
        iqr = q[2] - q[0]
    else:
        iqr = ordered[-1] - ordered[0]
    p95_idx = min(len(ordered) - 1, max(0, int(round(0.95 * (len(ordered) - 1)))))
    return {
        "count": float(len(ordered)),
        "min_ms": ordered[0],
        "median_ms": median,
        "p95_ms": ordered[p95_idx],
        "max_ms": ordered[-1],
        "iqr_ms": iqr,
    }


def compare_series(
    baseline_ms: list[float],
    candidate_ms: list[float],
    *,
    min_delta_pct: float = 2.0,
) -> dict[str, object]:
    base = summarize(baseline_ms)
    cand = summarize(candidate_ms)
    delta_ms = cand["median_ms"] - base["median_ms"]
    delta_pct = (delta_ms / base["median_ms"] * 100.0) if base["median_ms"] > 0 else 0.0
    # Noise floor: combined half-IQR plus a relative floor.
    noise_floor_ms = max(
        (base["iqr_ms"] + cand["iqr_ms"]) * 0.5,
        base["median_ms"] * (min_delta_pct / 100.0),
    )
    significant = abs(delta_ms) > noise_floor_ms
    verdict = "neutral"
    if significant:
        verdict = "faster" if delta_ms < 0 else "slower"
    return {
        "baseline": base,
        "candidate": cand,
        "delta_ms": delta_ms,
        "delta_pct": delta_pct,
        "noise_floor_ms": noise_floor_ms,
        "significant": significant,
        "verdict": verdict,
    }


def p_core_prefix(cores: str | None) -> list[str]:
    if not cores or platform.system() != "Linux":
        return []
    taskset = shutil.which("taskset")
    if not taskset:
        return []
    return [taskset, "-c", cores]


def time_command(argv: list[str], *, cwd: Path) -> float:
    started = time.perf_counter()
    completed = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, check=False)
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()[-400:]
        raise RuntimeError(f"benchmark command exited {completed.returncode}: {detail}")
    return elapsed_ms


def run_comparison(
    base_cmd: list[str],
    cand_cmd: list[str],
    *,
    rounds: int,
    warmup: int,
    cwd: Path,
    p_cores: str | None = None,
    quiet: bool = False,
    min_delta_pct: float = 2.0,
    base_binary: Path | None = None,
    cand_binary: Path | None = None,
) -> dict[str, object]:
    prefix = p_core_prefix(p_cores)
    base_argv = [*prefix, *base_cmd]
    cand_argv = [*prefix, *cand_cmd]
    quiet_enabled = False
    try:
        if quiet and BUILD_SLOT.is_file():
            subprocess.run([sys.executable, "-B", str(BUILD_SLOT), "quiet-on"], cwd=ROOT, check=True)
            quiet_enabled = True
        for _ in range(warmup):
            time_command(base_argv, cwd=cwd)
            time_command(cand_argv, cwd=cwd)
        baseline_samples: list[float] = []
        candidate_samples: list[float] = []
        for idx in range(rounds):
            # Alternate A/B vs B/A order each round to cancel thermal/cache drift.
            if idx % 2 == 0:
                baseline_samples.append(time_command(base_argv, cwd=cwd))
                candidate_samples.append(time_command(cand_argv, cwd=cwd))
            else:
                candidate_samples.append(time_command(cand_argv, cwd=cwd))
                baseline_samples.append(time_command(base_argv, cwd=cwd))
    finally:
        if quiet_enabled and BUILD_SLOT.is_file():
            subprocess.run([sys.executable, "-B", str(BUILD_SLOT), "quiet-off"], cwd=ROOT, check=False)

    report = compare_series(baseline_samples, candidate_samples, min_delta_pct=min_delta_pct)
    report["rounds"] = rounds
    report["warmup"] = warmup
    report["p_cores"] = p_cores if prefix else None
    report["baseline_samples_ms"] = baseline_samples
    report["candidate_samples_ms"] = candidate_samples
    if base_binary and base_binary.is_file():
        report["baseline_sha256"] = sha256_file(base_binary)
    if cand_binary and cand_binary.is_file():
        report["candidate_sha256"] = sha256_file(cand_binary)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Deterministic alternating release benchmark comparison driver.")
    parser.add_argument("--base-cmd", required=True, help="Baseline command string (split via shlex)")
    parser.add_argument("--cand-cmd", required=True, help="Candidate command string (split via shlex)")
    parser.add_argument("--base-binary", type=Path, default=None, help="Optional baseline binary path for SHA-256")
    parser.add_argument("--cand-binary", type=Path, default=None, help="Optional candidate binary path for SHA-256")
    parser.add_argument("--rounds", type=int, default=7, help="Measured alternating rounds (default: 7)")
    parser.add_argument("--warmup", type=int, default=1, help="Unmeasured warmup rounds (default: 1)")
    parser.add_argument("--p-cores", default=None, help="Optional Linux CPU list for taskset (e.g. 4-11)")
    parser.add_argument("--quiet", action="store_true", help="Acquire build_slot.py quiet-on/quiet-off around runs")
    parser.add_argument("--min-delta-pct", type=float, default=2.0, help="Minimum percent noise floor (default: 2.0)")
    parser.add_argument("--json", action="store_true", help="Emit JSON summary to stdout")
    parser.add_argument("--cwd", type=Path, default=ROOT, help="Working directory")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.rounds < 1:
        print("error: --rounds must be >= 1", file=sys.stderr)
        return 2
    report = run_comparison(
        shlex.split(args.base_cmd),
        shlex.split(args.cand_cmd),
        rounds=args.rounds,
        warmup=args.warmup,
        cwd=args.cwd,
        p_cores=args.p_cores,
        quiet=args.quiet,
        min_delta_pct=args.min_delta_pct,
        base_binary=args.base_binary,
        cand_binary=args.cand_binary,
    )
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        base = report["baseline"]
        cand = report["candidate"]
        print(
            f"baseline median={base['median_ms']:.2f}ms (iqr={base['iqr_ms']:.2f}ms) | "
            f"candidate median={cand['median_ms']:.2f}ms (iqr={cand['iqr_ms']:.2f}ms) | "
            f"delta={report['delta_ms']:+.2f}ms ({report['delta_pct']:+.2f}%) | "
            f"verdict={report['verdict']} (significant={report['significant']})"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
