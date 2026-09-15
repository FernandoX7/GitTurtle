#!/usr/bin/env python3
"""Summarize recorded app observations; this never launches or controls the app."""

import json
import math
from pathlib import Path
import re
import statistics
import sys


def summarize(path):
    rows = []
    for line in Path(path).read_text().splitlines():
        match = re.search(r"gitturtle.scroll kind=(\w+) time_us=(\d+)", line)
        if match:
            rows.append((match[1], int(match[2]), line))
    inputs = [row for row in rows if row[0] == "input"][:240]
    if len(inputs) != 240:
        raise ValueError("Expected 240 recorded input observations")
    lower, upper = inputs[0][1], inputs[-1][1] + 500_000
    paints = [row for row in rows if row[0] == "split_paint" and lower <= row[1] <= upper]

    def offsets(line, side):
        match = re.search(side + r"_offset=Point \{ x: ([-\d.]+)px, y: ([-\d.]+)px \}", line)
        if not match:
            raise ValueError("Missing offset pair")
        return [float(match[1]), float(match[2])]

    def intervals(observations):
        values = [(b[1] - a[1]) / 1000 for a, b in zip(observations, observations[1:])]
        ordered = sorted(values)
        return {
            "samples_ms": values,
            "count": len(values),
            "p50_nearest_rank_ms": ordered[math.ceil(len(values) * 0.50) - 1],
            "p95_nearest_rank_ms": ordered[math.ceil(len(values) * 0.95) - 1],
            "median_ms": statistics.median(values),
            "max_ms": max(values),
        }

    frames = [{"time_us": row[1], "before": offsets(row[2], "before"),
               "after": offsets(row[2], "after")} for row in paints]
    return {
        "selection": "First 240 input observations; split paints from first input through last input + 500 ms, inclusive",
        "window_us": [lower, upper],
        "input_count": len(inputs),
        "input_times_us": [row[1] for row in inputs],
        "input_span_ms": (inputs[-1][1] - inputs[0][1]) / 1000,
        "input_intervals": intervals(inputs),
        "split_paint_count": len(frames),
        "split_paint_mismatches": sum(frame["before"] != frame["after"] for frame in frames),
        "split_paint_intervals": intervals(paints),
        "split_paints": frames,
    }


if __name__ == "__main__":
    print(json.dumps({Path(path).name: summarize(path) for path in sys.argv[1:]}, indent=2))
