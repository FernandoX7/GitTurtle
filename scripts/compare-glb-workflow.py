#!/usr/bin/env python3
"""Compare all exported coordinates and independent unlit pixel samples.

Reads only explicit evidence files. Mismatches produce a nonzero exit status;
refusals stay separate from compared results. No asset loading or rendering.
"""
import argparse
import json
import math
from pathlib import Path
import struct


def read_jsonl(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def coordinates(directory, sample):
    raw = (directory / sample["triangle_file"]).read_bytes()
    return struct.unpack("<" + "d" * (len(raw) // 8), raw)


def compare(expected, actual, absolute, relative):
    if len(expected) != len(actual):
        return {"ok": False, "expected_coordinates": len(expected), "actual_coordinates": len(actual)}
    maximum = max((abs(x - y) for x, y in zip(expected, actual)), default=0.)
    mismatch = sum(not math.isclose(x, y, abs_tol=absolute, rel_tol=relative) for x, y in zip(expected, actual))
    return {"ok": mismatch == 0, "coordinates": len(expected), "maximum_absolute_mm": maximum, "mismatched_coordinates": mismatch}


def key(sample):
    return (sample.get("clip"), sample.get("time"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference_jsonl", type=Path)
    parser.add_argument("actual_jsonl", type=Path)
    parser.add_argument("reference_directory", type=Path)
    parser.add_argument("actual_directory", type=Path)
    parser.add_argument("--authored", type=Path)
    parser.add_argument("--absolute-mm", type=float, default=.001)
    parser.add_argument("--relative", type=float, default=1e-6)
    parser.add_argument("--pixel-tolerance", type=int, default=1)
    args = parser.parse_args()
    reference, actual = read_jsonl(args.reference_jsonl), read_jsonl(args.actual_jsonl)
    actual_by_name = {row["name"]: row for row in actual}
    report = {"absolute_mm_tolerance": args.absolute_mm, "relative_tolerance": args.relative,
              "pixel_channel_tolerance": args.pixel_tolerance, "comparisons": [], "refusals": [], "authored": []}
    for expected in reference:
        found = actual_by_name.get(expected["name"])
        if not expected.get("ok") or not found or not found.get("ok"):
            report["refusals"].append({"name": expected["name"], "reference_ok": expected.get("ok"),
                                       "actual_ok": found.get("ok") if found else None,
                                       "reference_error": expected.get("error"), "actual_error": found.get("error") if found else None})
            continue
        found_samples = {key(sample): sample for sample in found["samples"]}
        for sample in expected["samples"]:
            found_sample = found_samples.get(key(sample))
            row = {"name": expected["name"], "clip": sample["clip"], "time": sample["time"]}
            if not found_sample or "triangle_file" not in found_sample:
                row.update(ok=False, error="Missing production sample or triangle export")
            else:
                row.update(compare(coordinates(args.reference_directory, sample), coordinates(args.actual_directory, found_sample), args.absolute_mm, args.relative))
                reference_pixels = [pixel for pixel in sample.get("appearance_samples", []) if not pixel.get("unsupported")]
                actual_pixels = {tuple(pixel["pixel"]): pixel for pixel in found_sample.get("appearance_samples", [])}
                pixel_differences = []
                for pixel in reference_pixels:
                    match = actual_pixels.get(tuple(pixel["pixel"]))
                    maximum = max(abs(x - y) for x, y in zip(pixel["rgba"], match["rgba"])) if match else None
                    pixel_differences.append({"pixel": pixel["pixel"], "expected_rgba": pixel["rgba"],
                                              "actual_rgba": match["rgba"] if match else None,
                                              "maximum_channel_difference": maximum,
                                              "ok": maximum is not None and maximum <= args.pixel_tolerance})
                if pixel_differences:
                    row["pixels"] = pixel_differences
                    row["ok"] = row["ok"] and all(pixel["ok"] for pixel in pixel_differences)
            report["comparisons"].append(row)
    if args.authored:
        authored = json.loads(args.authored.read_text())
        refs = {row["name"]: row for row in reference if row.get("ok")}
        for expected in authored["samples"]:
            if expected["file"] not in refs:
                continue
            found = next((sample for sample in refs[expected["file"]]["samples"] if key(sample) == (expected["clip"], expected["time"])), None)
            if not found:
                continue
            wanted = [coordinate for index in authored["indices"] for coordinate in expected["vertices_mm"][index]]
            report["authored"].append({"name": expected["file"], "clip": expected["clip"], "time": expected["time"],
                **compare(wanted, coordinates(args.reference_directory, found), 1e-8, 1e-10)})
    report["unexpected_refusals"] = sum(row["reference_ok"] and not row["actual_ok"] for row in report["refusals"])
    report["ok"] = bool(report["comparisons"]) and not report["unexpected_refusals"] and all(row["ok"] for row in report["comparisons"] + report["authored"])
    report["matched_samples"] = sum(row["ok"] for row in report["comparisons"])
    report["failed_samples"] = sum(not row["ok"] for row in report["comparisons"])
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if report["ok"] else 1)


if __name__ == "__main__":
    main()
