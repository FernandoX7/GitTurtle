#!/usr/bin/env python3
"""Build and record opt-in release app-helper benchmarks without launching GPUI."""
import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
TEST = "working_selection::benchmarks::prepared_review_and_cached_paths_release_benchmark"


def run(args, **kwargs):
    result = subprocess.run(args, cwd=ROOT, text=True, capture_output=True, **kwargs)
    if result.returncode:
        sys.stderr.write(result.stdout + result.stderr)
        result.check_returncode()
    return result


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compiled_inputs():
    """Hash a conservative source-provenance superset, including local patches."""
    paths = set(ROOT.glob("crates/**/*.rs")) | set(ROOT.glob("crates/**/Cargo.toml"))
    paths.update(path for path in ROOT.glob("assets/**/*") if path.is_file())
    paths.update(path for path in ROOT.glob("crates/**/tests/fixtures/**/*") if path.is_file())
    paths.update(ROOT / name for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml") if (ROOT / name).exists())
    # Cargo.lock does not hash path-patched dependencies. Keep their complete
    # supplied trees (Rust, manifests, licenses, patch notes and embedded data),
    # including future non-Rust assets, without recording local build caches.
    for directory, directories, files in os.walk(ROOT / "vendor"):
        directories[:] = [name for name in directories if name not in {"target", ".git", "__pycache__"}]
        paths.update(
            Path(directory) / name for name in files
            if name != ".DS_Store" and not name.endswith((".pyc", ".pyo"))
            and (Path(directory) / name).is_file()
        )
    return {str(path.relative_to(ROOT)): digest(path) for path in sorted(paths)}


def observation():
    result = {"utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "load_average": os.getloadavg()}
    for key, command in [
        ("uptime", ["uptime"]),
        ("processes_by_cpu", ["ps", "-Ao", "pid,comm,%cpu,rss", "-r"]),
    ]:
        try:
            result[key] = run(command).stdout.splitlines()[:12]
        except subprocess.CalledProcessError as error:
            result[key] = {"unavailable": error.stderr}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="Raw JSON destination; report uses the same stem with .md")
    args = parser.parse_args()
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    before = compiled_inputs()
    build = run(["cargo", "test", "--locked", "--release", "-p", "gitturtle", "--no-run", "--message-format=json"])
    binary = None
    for line in build.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == "gitturtle" and message.get("profile", {}).get("test") and message.get("executable"):
            binary = Path(message["executable"])
    if binary is None:
        raise RuntimeError("Cargo did not identify the release test executable")
    after_build = compiled_inputs()
    if before != after_build:
        raise RuntimeError("Compiled inputs changed during build; rerun when edits finish")
    print(f"Release test built: {binary.name}", flush=True)
    environment = os.environ.copy()
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
    for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS"]:
        environment.pop(key, None)
    start = observation()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="gitturtle-app-review-bench-") as temporary:
        temporary = Path(temporary)
        fixture = temporary / "seed"
        fixture.mkdir()
        run(["git", "init", "--quiet", str(fixture)], env=environment)
        (fixture / "untracked.txt").write_text("Disposable status seed\n")
        fixture_index_before = (fixture / ".git/index").exists()
        result_path = temporary / "samples.json"
        environment.update(GITTURTLE_REVIEW_BENCH_OUTPUT=str(result_path), GITTURTLE_REVIEW_BENCH_FIXTURE=str(fixture))
        command = [str(binary), "--ignored", "--exact", TEST, "--test-threads=1", "--nocapture"]
        # /usr/bin/time observes the already-built process, so compiler RSS is
        # excluded. macOS reports max RSS in bytes; Linux reports KiB.
        if platform.system() == "Darwin":
            invocation = ["/usr/bin/time", "-l", *command]
        else:
            invocation = ["/usr/bin/time", "-v", *command]
        child = run(invocation, env=environment)
        data = json.loads(result_path.read_text())
        data["test_output"] = child.stdout
        data["process_resource_observation"] = child.stderr
        data["seed_preservation"] = {"untracked_bytes_preserved": (fixture / "untracked.txt").read_text() == "Disposable status seed\n", "index_presence_unchanged": (fixture / ".git/index").exists() == fixture_index_before}
        assert all(data["seed_preservation"].values())
    elapsed = time.monotonic() - started
    end = observation()
    after = compiled_inputs()
    if before != after:
        raise RuntimeError("Compiled inputs changed during sampling; no evidence published")
    machine = {"platform": platform.platform(), "machine": platform.machine(), "processors": os.cpu_count()}
    if platform.system() == "Darwin":
        for name in ["machdep.cpu.brand_string", "hw.model", "hw.memsize", "hw.logicalcpu"]:
            machine[name] = run(["sysctl", "-n", name]).stdout.strip()
        machine["macos"] = run(["sw_vers"]).stdout
    for name in ["rustc", "cargo", "git"]:
        machine[name] = run([name, "--version"]).stdout.strip()
    data.update({
        "build": {"command": "cargo test --locked --release -p gitturtle --no-run --message-format=json", "git_head": run(["git", "rev-parse", "HEAD"]).stdout.strip(), "compiled_input_sha256": before, "compiled_input_map_sha256": hashlib.sha256(json.dumps(before, sort_keys=True, separators=(",", ":")).encode()).hexdigest(), "input_maps_equal_before_build_after_build_after_run": True, "binary_sha256": digest(binary), "binary_name": binary.name, "runner_sha256": digest(Path(__file__)), "harness_sha256": digest(ROOT / "crates/app/src/review_bench.rs")},
        "hardware_software": machine, "observations": {"before": start, "after": end}, "elapsed_seconds_including_seed_setup": elapsed,
    })
    for series in data["series"]:
        values = sorted(sample["milliseconds"] for sample in series["attempts"] if not sample["warmup"])
        rank = lambda percentile: values[max(0, math.ceil(len(values) * percentile) - 1)]
        series["summary_ms"] = {"minimum": values[0], "median": statistics.median(values), "p50_nearest_rank": rank(.5), "p95_nearest_rank": rank(.95), "p99_nearest_rank": rank(.99), "maximum": values[-1]}
    output.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    report = [
        "# App review preparation and path-filter measurements",
        "",
        "Current release CPU costs for the production preparation/filter helpers. These measurements do not establish a speedup, native frame latency, queue responsiveness, or older-hardware performance.",
        "",
        f"[Raw JSON]({output.name}) retains every warmup and sample, stable result dimensions, memory observations, complete compiled-input hashes, and hardware/load data. Reproduce with `python3 scripts/bench-app-review.py --output <raw.json>`; the runner creates and removes its disposable seed repository and builds a release test executable. The ignored test never launches GPUI.",
        "",
        "## Execution and identity", "",
        f"- HEAD: `{data['build']['git_head']}` plus the exact source hashes in the JSON. The only benchmark hooks are compiled under `cfg(test)`.",
        f"- Compiled-input map SHA-256: `{data['build']['compiled_input_map_sha256']}`; all {len(before)} input hashes matched before build, after build and after sampling. Cargo.lock pins external dependencies; the map covers workspace manifests, Rust sources and embedded assets.",
        f"- Release test executable SHA-256: `{data['build']['binary_sha256']}`.",
        f"- UTC interval: {start['utc']}–{end['utc']}; {elapsed:.3f} seconds including disposable seed setup.",
        f"- Hardware/software: {machine.get('machdep.cpu.brand_string', machine['machine'])}, {machine.get('hw.model', '')}, {machine['processors']} logical CPUs, {int(machine.get('hw.memsize', 0)) / 2**30:.0f} GiB RAM; {machine['platform']}; {machine['rustc']}; {machine['git']}.",
        f"- Background load was not isolated. One-minute load averages were {start['load_average'][0]:.2f} before and {end['load_average'][0]:.2f} after. Process snapshots are in the JSON.",
        "",
        "## Fixtures and boundaries", "",
        data["fixture"]["description"],
        "",
        "Text sources have 20,000 lines per side and 201 change blocks: 200 adjacent substantive/whitespace-only replacement pairs and one final-line replacement. Mixed LF/CRLF endings and the missing final newline are preserved. The ordinary source and patch buffers, their immutable prepared presentation, and both 50,000-path indexes are created before timing. One separate patch contains a 24 KiB line on each side to exercise the bounded intraline fallback.",
        "",
        "Historical indexes normalize both old/new paths; working indexes normalize current/original paths. The selective query is `module_017/` (500 matching entries), broad query `.rs` (50,000), absent query `missing_review_path` (zero), and renamed-history query `ancien_` (7,143). Working samples include matching, production staged/unstaged row construction, optional directory sorting, and total staged/conflict summaries. Source row indices remain operation identities. No selected-file content or Git status reads are timed.",
        "",
        data["boundary"], "", data["cache"],
        "",
        "Each series has three warmups and forty measured calls. All result dimensions/fingerprints matched within each series. Percentiles use nearest ranks; p99 equals the maximum with forty samples. The conventional median is also retained in JSON. No outlier was removed.",
        "",
        "## Results", "",
        "| Production helper series | p50 ms | p95 ms | p99 / max ms |", "| --- | ---: | ---: | ---: |",
    ]
    for series in data["series"]:
        summary = series["summary_ms"]
        report.append(f"| {series['name']} | {summary['p50_nearest_rank']:.3f} | {summary['p95_nearest_rank']:.3f} | {summary['maximum']:.3f} |")
    report.extend(["", "## Memory and limitations", "", f"Release-test process RSS from `ps` was {data['process_rss_kib']['before_fixture']:,} KiB before fixture construction, {data['process_rss_kib']['after_fixture']:,} KiB afterward, and {data['process_rss_kib']['after_all_samples']:,} KiB after all samples. The raw JSON also retains `/usr/bin/time` peak-RSS and resource statistics for the already-built test process; compilation is excluded. Loaded libraries, synthetic fixtures, cached indexes and allocator-retained allocations are included. These are process observations, not editor/GPU measurements or a product memory cap.", "", "The seed file and index presence were unchanged. Inputs and result validation warm caches; no filesystem flush was attempted. Samples exclude queue scheduling/cancellation latency, main-thread callbacks, editor construction, rendering and OS presentation, image decoding, native interaction and actual repository path discovery. Rapid-selection memory growth, Linux and other hardware remain outside this record.", ""])
    output.with_suffix(".md").write_text("\n".join(report))
    print(f"Recorded {len(data['series'])} series to {output}", flush=True)


if __name__ == "__main__":
    main()
