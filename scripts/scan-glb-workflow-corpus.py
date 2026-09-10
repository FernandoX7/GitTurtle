#!/usr/bin/env python3
"""Run the release probe over private captures and emit sanitized coverage.

The raw probe log stays private (it includes authored clip names). The sanitized
JSONL uses only ordinal labels/hashes and must be reviewed before publication.
Build the probe immediately before invocation; source hashes are recorded here.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_hashes(checkout):
    paths = sorted((checkout / "crates/preview").glob("**/*.rs"))
    paths += [checkout / "Cargo.lock", checkout / "crates/preview/Cargo.toml"]
    return {str(path.relative_to(checkout)): sha(path) for path in paths}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("probe", type=Path)
    parser.add_argument("inventory", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    checkout = Path(__file__).resolve().parents[1]
    destination = args.destination.resolve()
    if destination.exists() or checkout == destination or checkout in destination.parents:
        parser.error("Choose a new private evidence destination outside checkout")
    destination.mkdir(parents=True)
    entries = json.loads(args.inventory.read_text())["entries"]
    started = datetime.now(timezone.utc).isoformat()
    sources_before = source_hashes(checkout)
    executable_hash = sha(args.probe)
    command = [str(args.probe.resolve()), "--corpus", str(destination / "probe-output")]
    command += [str(args.inventory.parent / "captures" / f"{entry['id']}.glb") for entry in entries]
    with (destination / "raw-private.jsonl").open("w") as log:
        subprocess.run(command, stdout=log, check=True)
    rows = [json.loads(line) for line in (destination / "raw-private.jsonl").read_text().splitlines()]
    if len(rows) != len(entries):
        raise RuntimeError("Probe did not return every input")
    outcomes = []
    for entry, row in zip(entries, rows):
        if row["name"] != f"{entry['id']}.glb":
            raise RuntimeError("Probe order differed from capture inventory")
        details = row.get("details", [])
        samples = row.get("samples", [])
        default = samples[0] if samples else {}
        failed_frames = [{"clip": sample.get("clip"), "time": sample.get("time"),
                          "evaluate_error": sample.get("error"), "renders": [render for render in sample.get("renders", []) if not render["ok"]]}
                         for sample in samples if sample.get("error") or any(not render["ok"] for render in sample.get("renders", []))]
        outcomes.append({"id": entry["id"], "sha256": entry["sha256"], "bytes": entry["bytes"],
                         "classification": row["classification"], "error": row.get("error"),
                         "triangles": default.get("triangles"), "retained_bytes": default.get("retained_bytes"),
                         "source_has_skin": bool(entry.get("skins")), "source_has_morph": bool(entry.get("morph_primitives")),
                         "source_has_animation": bool(entry.get("clips")), "supported_clips": len(row.get("clips", [])),
                         "appearance_approximation": any("approximation" in detail.lower() or "not reproduced" in detail for detail in details),
                         "omitted_texture": any("texture omitted" in detail.lower() for detail in details),
                         "sampled_frames": len(samples), "failed_frames": failed_frames})
    with (destination / "corpus-sanitized.jsonl").open("w") as log:
        for row in outcomes:
            log.write(json.dumps(row, separators=(",", ":")) + "\n")
    summary = {"started_at": started, "finished_at": datetime.now(timezone.utc).isoformat(),
               "files": len(entries), "bytes": sum(entry["bytes"] for entry in entries),
               "classification": dict(Counter(row["classification"] for row in outcomes)),
               "skinned_supported": sum(row["source_has_skin"] and row["classification"] == "supported" for row in outcomes),
               "morph_supported": sum(row["source_has_morph"] and row["classification"] == "supported" for row in outcomes),
               "animated_supported": sum(bool(row["supported_clips"]) for row in outcomes),
               "supported_clips": sum(row["supported_clips"] for row in outcomes),
               "appearance_approximation": sum(row["appearance_approximation"] for row in outcomes),
               "omitted_texture": sum(row["omitted_texture"] for row in outcomes),
               "sampled_frames": sum(row["sampled_frames"] for row in outcomes),
               "models_with_failed_frames": sum(bool(row["failed_frames"]) for row in outcomes),
               "decode_refusals": dict(Counter(row["error"] for row in outcomes if row["error"])),
               "sample_policy": "Authored default and midpoint of every supported clip; each has fitted Front360solid,720solid,720edges.",
               "evidence_scope": "Supplied-byte release CPU observations; not native latency or peak memory",
               "probe_sha256": executable_hash, "raw_private_sha256": sha(destination / "raw-private.jsonl"),
               "sanitized_sha256": sha(destination / "corpus-sanitized.jsonl"),
               "source_hashes": sources_before, "sources_unchanged_during_scan": sources_before == source_hashes(checkout)}
    (destination / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({key: value for key, value in summary.items() if key != "source_hashes"}, indent=2))


if __name__ == "__main__":
    main()
