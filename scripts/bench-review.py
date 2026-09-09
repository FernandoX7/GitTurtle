#!/usr/bin/env python3
"""Build and measure passive review APIs in a newly created disposable fixture.

The only repository mutations occur in a fresh tempfile directory during setup.
The release Rust harness performs reads only. The output includes raw attempts,
source/build provenance and byte/mode/mtime inventory checks around measurement.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import stat
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BASELINE = "e85883bbf42284d0cd695c602b3d3ad265cfbb49"
GIT_ENV = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT="0", GIT_LFS_SKIP_SMUDGE="1", LC_ALL="C")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def run(args, *, cwd=None, data=None, env=None, check=True):
    return subprocess.run(args, cwd=cwd, input=data, env=env, capture_output=True, check=check)


def git(path, *args, data=None):
    return run(["git", "-C", str(path), *args], data=data, env=GIT_ENV).stdout


def git_path(path):
    return b'"' + b"".join(bytes([byte]) if 32 <= byte < 127 and byte not in (34, 92)
                          else ("\\%03o" % byte).encode() for byte in path.encode()) + b'"'


def create_fixture(folder):
    repository = folder / "work"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    for key, value in [("user.name", "Review Benchmark"), ("user.email", "review@example.invalid"),
                       ("commit.gpgsign", "false"), ("core.hooksPath", ".git/hooks"),
                       ("core.logAllRefUpdates", "true"), ("gc.auto", "0")]:
        git(repository, "config", key, value)
    absent_object = b"deliberately absent fixture object\n" * 1024
    pointer = ("version https://git-lfs.github.com/spec/v1\noid sha256:" +
               sha(absent_object) + f"\nsize {len(absent_object)}\n").encode()
    stream = bytearray()

    def commit(reference, mark, sequence, parent=None):
        message = f"Review fixture commit {sequence}\n".encode()
        stream.extend(f"commit {reference}\nmark :{mark}\ncommitter Review Benchmark <review@example.invalid> {1700000000 + sequence} +0000\ndata {len(message)}\n".encode())
        stream.extend(message)
        if parent is not None:
            stream.extend(f"from :{parent}\n".encode())

    def write(path, content, mode="100644"):
        stream.extend(f"M {mode} inline ".encode() + git_path(path) + f"\ndata {len(content)}\n".encode())
        stream.extend(content + b"\n")

    def path(index):
        return f"src/group{index % 64:02}/module_{index:05}.rs"

    commit("refs/heads/main", 1, 0)
    for index in range(12000):
        write(path(index), f"// module {index}\npub fn value() -> usize {{ {index} }}\n".encode())
    write("docs/Unicode é/turtle 🐢.md", "# Turtle 🐢\nUnicode path fixture.\n".encode())
    write("src/rename-before.rs", b"pub fn renamed() -> bool { true }\n")
    write("assets/pixels.bin", bytes(range(256)) * 4)
    write("assets/turtle-pointer.bin", pointer)
    write("docs/link", b"Unicode \xc3\xa9/turtle \xf0\x9f\x90\xa2.md", "120000")
    stream.extend(b"\n")
    for sequence in range(1, 61):
        commit("refs/heads/main", sequence + 1, sequence)
        for index in range((sequence - 1) * 20, sequence * 20):
            write(path(index), f"// reviewed module {index}\npub fn value() -> usize {{ {sequence * 1000 + index} }}\n".encode())
        if sequence == 30:
            stream.extend(b"D src/rename-before.rs\n")
            write("src/renamed-after.rs", b"pub fn renamed() -> bool { true }\n")
        if sequence == 50:
            write("assets/pixels.bin", bytes(reversed(range(256))) * 4)
        stream.extend(b"\n")
    commit("refs/heads/review-before", 100, 100, parent=21)
    write("side-only.txt", b"Only on the comparison's before branch.\n")
    write(path(0), b"// diverged before branch\npub fn value() -> usize { 99999 }\n")
    stream.extend(b"\nreset refs/heads/rebase-base\nfrom :11\n\nreset refs/remotes/origin/main\nfrom :61\n\ndone\n")
    git(repository, "fast-import", "--quiet", data=bytes(stream))
    git(repository, "reset", "--hard", "main")
    head = git(repository, "rev-parse", "HEAD").decode().strip()
    previous = git(repository, "rev-parse", "HEAD^").decode().strip()
    # One process performs ordinary Git ref transactions, retaining actual reflog
    # semantics. The final ref returns to the same reviewed HEAD before timing.
    transactions = bytearray()
    for index in range(1200):
        target = previous if index % 2 == 0 else head
        transactions.extend(f"start\nupdate HEAD {target}\nprepare\ncommit\n".encode())
    git(repository, "update-ref", "--stdin", "--create-reflog", "-m", "disposable review benchmark checkpoint", data=bytes(transactions))
    remote = folder / "local-origin.git"
    run(["git", "init", "--bare", str(remote)], env=GIT_ENV)
    git(repository, "remote", "add", "origin", str(remote))
    git(repository, "config", "lfs.url", remote.as_uri() + "/info/lfs")
    linked = folder / "linked"
    git(repository, "worktree", "add", "-b", "bench-linked", str(linked), "main")
    for index in range(4):
        with (linked / path(index)).open("ab") as file:
            file.write(b"// unrelated linked-worktree edit\n")
    for index in range(3):
        (linked / f"untracked-{index}.txt").write_text("Keep this local fixture data.\n")
    (repository / ".git/info/exclude").write_text("build/\n")
    (linked / "build").mkdir()
    (linked / "build/cache.bin").write_bytes(b"ignored fixture data\0")
    git(repository, "status", "--porcelain=v1")
    return repository, {
        "tracked_files": 12005, "main_commits": 61, "linear_rebase_commits": 50,
        "worktrees": 2, "reflog_transactions": 1200,
        "linked_worktree_tracked_edits": 4, "linked_worktree_untracked_files": 3,
        "linked_worktree_ignored_files": 1,
        "head": head,
        "before": git(repository, "rev-parse", "review-before").decode().strip(),
        "merge_base": git(repository, "merge-base", "main", "review-before").decode().strip(),
        "rebase_base": git(repository, "rev-parse", "rebase-base").decode().strip(),
        "fast_import_stream_sha256": sha(stream), "lfs_pointer_sha256": sha(pointer),
        "git_config_isolation": "system/global configuration excluded; local fixture configuration retained",
        "network": "none; origin is an empty local bare fixture; LFS preparation only",
    }


def sources():
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "crates/git-core/Cargo.toml",
             ROOT / "crates/git-core/examples/review_bench.rs"]
    paths.extend(sorted((ROOT / "crates/git-core/src").rglob("*.rs")))
    hashes = {str(path.relative_to(ROOT)): sha(path.read_bytes()) for path in paths}
    diff = run(["git", "diff", "--no-ext-diff", "--binary", BASELINE, "--", "Cargo.toml", "Cargo.lock", "crates/git-core/Cargo.toml", "crates/git-core/src", "crates/git-core/examples/review_bench.rs"], cwd=ROOT).stdout
    return {
        "baseline": BASELINE,
        "head": run(["git", "rev-parse", "HEAD"], cwd=ROOT).stdout.decode().strip(),
        "files_sha256": hashes,
        "compiled_inputs_sha256": sha(json.dumps(hashes, sort_keys=True, separators=(",", ":")).encode()),
        "tracked_diff_from_baseline_sha256": sha(diff),
        "tracked_diff_bytes": len(diff),
        "untracked_compiled_inputs": [str(path.relative_to(ROOT)) for path in paths if run(["git", "ls-files", "--error-unmatch", str(path.relative_to(ROOT))], cwd=ROOT, check=False).returncode],
        "note": "The tracked diff hash excludes untracked files; every compiled source, including new files and the harness, has a content hash above.",
    }


def inventory(root):
    entries = {}
    for path in sorted(root.rglob("*")):
        metadata = path.lstat()
        kind = "symlink" if stat.S_ISLNK(metadata.st_mode) else "directory" if path.is_dir() else "file"
        content = os.fsencode(os.readlink(path)) if kind == "symlink" else path.read_bytes() if kind == "file" else b""
        entries[str(path.relative_to(root))] = (kind, stat.S_IMODE(metadata.st_mode), metadata.st_mtime_ns, len(content), sha(content))
    summary = {
        "entries": len(entries),
        "files": sum(entry[0] == "file" for entry in entries.values()),
        "directories": sum(entry[0] == "directory" for entry in entries.values()),
        "symlinks": sum(entry[0] == "symlink" for entry in entries.values()),
        "sha256": sha(json.dumps(entries, sort_keys=True, separators=(",", ":")).encode()),
    }
    return entries, summary


def conditions():
    def text(args):
        try:
            result = run(args, check=False)
        except FileNotFoundError:
            return "unavailable on this platform"
        return (result.stdout + result.stderr).decode(errors="replace").strip()
    processes = text(["ps", "-Ao", "pcpu,comm"]).splitlines()[1:]
    top = []
    for process in processes:
        fields = process.strip().split(None, 1)
        if len(fields) == 2:
            try:
                top.append((float(fields[0]), Path(fields[1]).name))
            except ValueError:
                pass
    return {
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": platform.platform(), "machine": platform.machine(),
        "hardware": text(["sysctl", "-n", "hw.model", "hw.ncpu", "hw.memsize", "machdep.cpu.brand_string"]),
        "os": text(["sw_vers"]), "load_average": os.getloadavg(),
        "top_process_cpu_percent_and_executable": sorted(top, reverse=True)[:8],
        "git": text(["git", "--version"]), "git_lfs": text(["git", "lfs", "version"]),
        "rustc": text(["rustc", "--version"]), "cargo": text(["cargo", "--version"]),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="New JSON report path outside the disposable fixture")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Report already exists; retain prior attempts and choose a new output path")
    temporary = Path(tempfile.mkdtemp(prefix="gitturtle-review-benchmark-"))
    fixture_root = temporary / "fixture"
    fixture_root.mkdir()
    print("Preparing fresh disposable fixture", flush=True)
    repository, fixture = create_fixture(fixture_root)
    provenance = sources()
    print("Building release benchmark", flush=True)
    build_started = time.monotonic()
    build = run(["cargo", "build", "--locked", "--release", "-p", "gitturtle-core", "--example", "review_bench"], cwd=ROOT, check=False)
    if build.returncode:
        raise RuntimeError(build.stderr.decode(errors="replace"))
    build_seconds = time.monotonic() - build_started
    compiled = sources()
    if compiled["compiled_inputs_sha256"] != provenance["compiled_inputs_sha256"]:
        raise RuntimeError("Core sources changed while compiling; no measurement was started")
    binary = ROOT / "target/release/examples/review_bench"
    binary_hash = sha(binary.read_bytes())
    before_entries, before = inventory(fixture_root)
    before_load = conditions()
    print("Measuring passive release calls", flush=True)
    started = time.monotonic()
    measured = run([str(binary), str(repository)], env=GIT_ENV, check=False)
    elapsed = time.monotonic() - started
    after_load = conditions()
    after_entries, after = inventory(fixture_root)
    final_sources = sources()
    records = [json.loads(line) for line in measured.stdout.decode().splitlines() if line]
    samples = [record for record in records if "series" in record]
    summaries = {}
    for name in sorted({sample["series"] for sample in samples}):
        attempts = [sample for sample in samples if sample["series"] == name and sample["phase"] == "measured"]
        values = sorted(sample["elapsed_ms"] for sample in attempts if sample["success"])
        nearest = lambda quantile: values[math.ceil(quantile * len(values)) - 1] if values else None
        summaries[name] = {"attempts": len(attempts), "successes": len(values), "failures": len(attempts) - len(values),
                           "median_ms": statistics.median(values) if values else None,
                           "p50_ms": nearest(.50), "p95_ms": nearest(.95), "p99_ms": nearest(.99),
                           "max_ms": values[-1] if values else None,
                           "result_changed": any(sample["result_changed"] for sample in attempts),
                           "result": next((sample["details"] for sample in attempts if sample["success"]), None)}
    changed = sorted(key for key in before_entries.keys() | after_entries.keys() if before_entries.get(key) != after_entries.get(key))
    report = {
        "purpose": "Current passive backend costs; no historical speedup or native-frame claim",
        "runner_sha256": sha(Path(__file__).read_bytes()), "source_at_compilation": provenance,
        "source_after_compilation": compiled, "source_after_measurement": final_sources,
        "compiled_inputs_unchanged": provenance["compiled_inputs_sha256"] == final_sources["compiled_inputs_sha256"],
        "build": {"command": "cargo build --locked --release -p gitturtle-core --example review_bench", "seconds": build_seconds, "binary_sha256": binary_hash, "output": (build.stdout + build.stderr).decode(errors="replace")},
        "conditions_before": before_load, "conditions_after": after_load,
        "cache_conditions": "One retained GitRepository/object reader; setup and inventory warm filesystem caches; 3 warmups per series; no filesystem flush and no application content cache. In-memory conflict text is created before timing.",
        "measurement_seconds": elapsed, "exit_code": measured.returncode,
        "stderr": measured.stderr.decode(errors="replace"), "fixture": fixture,
        "inventory_before": before, "inventory_after": after,
        "fixture_unchanged": not changed, "changed_inventory_paths": changed,
        "inventory_policy": "Compare entry kind, mode, byte contents, symlink target and mtime; exclude atime. Covers both worktrees, private/common Git metadata and the local bare remote.",
        "quantiles": "Nearest-rank p50/p95/p99; median averages middle ranks. With 40 successful samples p99 equals maximum. Errors never become zero samples.",
        "summaries": summaries, "raw_attempts": samples,
        "harness_validation": [record for record in records if "series" not in record],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Report saved: {args.output}", flush=True)
    print(json.dumps({name: {key: value for key, value in summary.items() if key in ("p50_ms", "p95_ms", "p99_ms", "max_ms", "failures")} for name, summary in summaries.items()}, indent=2))
    if measured.returncode or changed or not report["compiled_inputs_unchanged"]:
        raise RuntimeError("This attempt contains a failure or source/fixture drift; retain it without treating it as valid evidence")


if __name__ == "__main__":
    main()
