#!/usr/bin/env python3
"""Lifecycle of an optional, bounded cache on fresh GitHub-hosted Rust runners."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import time

PROFILES = {"debug", "release", "debug-release"}
MAX_FILES = 250_000
MAX_CLEAN_PACKAGES = 8
BUDGET_SECONDS = 90
SERIAL_LIMIT = 3 * 1024**3
SINGLE_PROFILE_LIMIT = 1536 * 1024**2


def run(command, *, cwd=None, timeout=120):
    result = subprocess.run(command, cwd=cwd, check=True, capture_output=True, timeout=timeout)
    if len(result.stdout) > 16 * 1024**2:
        raise ValueError("cache metadata exceeded its byte limit")
    return result.stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_identity(root):
    """Hash actual tracked build inputs, including local patches, not their names."""
    names = run(["git", "ls-files", "-z"], cwd=root).split(b"\0")
    hasher = hashlib.sha256()
    for raw in sorted(filter(None, names)):
        name = raw.decode("utf-8", errors="surrogateescape")
        path = Path(name)
        if not (name.startswith("vendor/") or name.startswith(".github/actions/setup-rust/")
                or path.name in {"Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml"}
                or name in {".cargo/config", ".cargo/config.toml"}):
            continue
        source = root / path
        hasher.update(raw + b"\0")
        if source.is_symlink():
            hasher.update(b"symlink\0" + os.fsencode(os.readlink(source)))
        else:
            with source.open("rb") as stream:
                while chunk := stream.read(1024 * 1024):
                    hasher.update(chunk)
        hasher.update(b"\0")
    return hasher.hexdigest()


def native_identity(platform):
    if platform == "Linux":
        # The complete installed package inventory includes transitive ABI inputs,
        # not just the names of explicitly requested development packages.
        commands = [["dpkg-query", "-W", "-f=${Package}:${Architecture}=${Version}\\n"],
                    ["clang", "--version"], ["cmake", "--version"], ["pkg-config", "--version"]]
    elif platform == "macOS":
        commands = [["xcodebuild", "-version"], ["xcrun", "--sdk", "macosx", "--show-sdk-version"],
                    ["xcrun", "--sdk", "macosx", "--show-sdk-build-version"],
                    ["xcrun", "clang", "--version"], ["sw_vers", "-buildVersion"]]
    else:
        raise ValueError("Rust cache supports only the native Quality platforms")
    return digest(b"\0".join(run(command) for command in commands))


def output_file(name, fields):
    with Path(os.environ[name]).open("a", encoding="utf-8") as stream:
        for key, value in fields.items():
            if "\n" in str(value) or "\r" in str(value):
                raise ValueError("multiline action output refused")
            stream.write(f"{key}={value}\n")


def roots():
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    cargo = temp / "gitturtle-cargo"
    if os.environ.get("CARGO_HOME") != str(cargo):
        raise ValueError("cache cleanup requires the action's isolated Cargo home")
    target = root / "target"
    if os.environ.get("CARGO_TARGET_DIR") != str(target):
        raise ValueError("cache cleanup requires the action's workspace target")
    paths = (target, cargo / "registry", cargo / "git")
    for path in (cargo, *paths):
        if path.is_symlink():
            raise ValueError("symlinked cache roots refused")
    return root, temp, paths


def entries(paths, *, deadline=None):
    """Conservative payload accounting: logical bytes + 4 KiB per entry.

    Counts aliases separately, and never follows directory/file symlinks. This is
    an upper bound before upstream dependency cleanup, not a compressed archive.
    Validate excluded source trees too; projection never bypasses these guards.
    """
    result = []
    for path in paths:
        if path.is_symlink():
            raise ValueError("symlinked cache root refused")
        if not path.exists():
            continue
        if not path.is_dir():
            raise ValueError("nondirectory cache root refused")

        def refused(error):
            raise error

        for directory, dirs, files in os.walk(path, followlinks=False, onerror=refused):
            if deadline is not None and time.monotonic() >= deadline:
                raise TimeoutError("cache accounting exceeded its time limit")
            for name in dirs + files:
                child = Path(directory) / name
                info = child.lstat()
                if not (stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode)):
                    raise ValueError("nonregular cache payload refused")
                result.append((child, (info.st_size if stat.S_ISREG(info.st_mode) else 0) + 4096))
                if len(result) > MAX_FILES:
                    raise ValueError("cache payload exceeded its entry limit")
    return result


def clear(paths):
    for path in paths:
        if path.is_symlink():
            raise ValueError("symlinked cache root refused")
        if path.exists():
            shutil.rmtree(path)


def largest_packages(packages, target_entries):
    by_name = {}
    for package in packages:
        names = {package["name"]}
        for target in package.get("targets", []):
            normalized = target["name"].replace("-", "_")
            names.update((normalized, "lib" + normalized))
        for name in names:
            by_name.setdefault(name, set()).add(package["id"])
    sizes = {}
    for path, size in target_entries:
        # Scores choose packages to pass to Cargo; Cargo owns actual artifact
        # cleanup (including fingerprints and build-script outputs).
        for component in path.parts:
            name = component.rsplit("-", 1)[0]
            for package_id in by_name.get(name, ()):
                sizes[package_id] = sizes.get(package_id, 0) + size
    return sorted(sizes, key=lambda package_id: (-sizes[package_id], package_id))


def removable_sources(packages, paths, measured):
    """Match only pinned rust-cache cleanRegistry's direct source directories.

    Current *-sys sources retain timestamps needed by native build scripts. Git
    databases and checkouts are never projected away. Keep extracted sources in
    place until the last Cargo command, which can otherwise recreate them.
    """
    keep = {f"{p['name']}-{p['version']}" for p in packages if p["name"].endswith("-sys")}
    source = paths[1] / "src"
    return {path for path, _ in measured
            if path.is_relative_to(source) and len(path.relative_to(source).parts) == 2
            and path.name not in keep and path.is_dir()}


def payload_snapshot(paths, measured, removable, stage):
    buckets = {name: {"logical_bytes": 0, "entry_overhead_bytes": 0, "entries": 0}
               for name in ("target", "registry_src_retained", "registry_src_removable",
                            "registry_cache", "registry_index", "registry_other",
                            "git_db", "git_checkouts", "git_other")}
    compiled = {name: {"files": 0, "logical_bytes": 0}
                for name in ("libraries", "native_generated", "fingerprints")}
    for path, size in measured:
        kind = None
        if path.is_relative_to(paths[0]):
            name = "target"
            parts = path.relative_to(paths[0]).parts
            if "deps" in parts and path.suffix in {".rlib", ".rmeta", ".a", ".so", ".dylib", ".dll"}:
                kind = "libraries"
            elif "build" in parts and "out" in parts:
                kind = "native_generated"
            elif ".fingerprint" in parts:
                kind = "fingerprints"
        elif path.is_relative_to(paths[1]):
            parts = path.relative_to(paths[1]).parts
            if parts[0] == "src":
                source_root = paths[1].joinpath(*parts[:3])
                name = "registry_src_removable" if source_root in removable else "registry_src_retained"
            else:
                name = "registry_" + parts[0] if parts[0] in {"cache", "index"} else "registry_other"
        else:
            parts = path.relative_to(paths[2]).parts
            name = "git_" + parts[0] if parts[0] in {"db", "checkouts"} else "git_other"
        bucket = buckets[name]
        bucket["logical_bytes"] += size - 4096
        bucket["entry_overhead_bytes"] += 4096
        bucket["entries"] += 1
        if kind and path.is_file():
            compiled[kind]["files"] += 1
            compiled[kind]["logical_bytes"] += size - 4096
    logical = sum(value["logical_bytes"] for value in buckets.values())
    overhead = sum(value["entry_overhead_bytes"] for value in buckets.values())
    excluded = buckets["registry_src_removable"]
    return {"stage": stage, "logical_bytes": logical, "entry_overhead_bytes": overhead,
            "entries": len(measured), "bytes": logical + overhead,
            "projected_bytes": logical + overhead - excluded["logical_bytes"] - excluded["entry_overhead_bytes"],
            "subtrees": buckets, "compiled": compiled}


def bound_payload(root, paths, profile, *, execute=run, snapshots=None):
    started = time.monotonic()
    deadline = started + BUDGET_SECONDS
    limit = SERIAL_LIMIT if profile == "debug-release" else SINGLE_PROFILE_LIMIT
    metadata = json.loads(execute(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1"], cwd=root, timeout=30))
    packages = metadata["packages"]
    snapshots = [] if snapshots is None else snapshots
    evictions = []

    def measure(stage):
        measured = entries(paths, deadline=deadline)
        removable = removable_sources(packages, paths, measured)
        snapshot = payload_snapshot(paths, measured, removable, stage)
        snapshots.append(snapshot)
        return measured, removable, snapshot

    measure("before_local_cleanup")
    local = [package["id"] for package in packages if package.get("source") is None]
    if local:
        execute(["cargo", "clean", "--locked", *[part for package_id in local for part in ("--package", package_id)]], cwd=root, timeout=30)
    measured, removable, snapshot = measure("after_local_cleanup")
    before = snapshot["bytes"]
    removed = 0
    ordered = largest_packages([p for p in packages if p.get("source") is not None],
                               [(p, size) for p, size in measured if p.is_relative_to(paths[0])])
    for package_id in ordered[:MAX_CLEAN_PACKAGES]:
        if snapshot["projected_bytes"] <= limit or time.monotonic() >= deadline:
            break
        execute(["cargo", "clean", "--locked", "--package", package_id], cwd=root, timeout=30)
        removed += 1
        index, package = next((index, package) for index, package in enumerate(packages) if package["id"] == package_id)
        eviction = {"metadata_index": index}
        # Never expose package IDs: Git IDs can include a source URL or path.
        for field in ("name", "version"):
            value = package.get(field)
            if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}", value):
                eviction[field] = value
        evictions.append(eviction)
        measured, removable, snapshot = measure(f"after_dependency_cleanup_{removed}")
    # Upstream repeats metadata discovery and this precise source pruning in its
    # post action. This physical rescan verifies the predicted payload before we
    # register that save. Upstream catches cleanup errors, so its actual archive
    # size still needs hosted evidence; this is a pre-registration bound. Never
    # prune before a Cargo command or split an OUT_DIR group.
    projected = snapshot["projected_bytes"]
    clear(sorted(removable))
    measured, _, snapshot = measure("after_source_pruning")
    if snapshot["bytes"] != projected:
        raise ValueError("cache source projection did not match physical pruning")
    dropped_target = snapshot["bytes"] > limit
    if dropped_target:
        clear(paths[:1])
        measured, _, snapshot = measure("after_target_fallback")
    retained = snapshot["bytes"]
    return {"limit_bytes": limit, "before_bytes": before, "retained_bytes": retained,
            "removed_dependency_packages": removed, "removed_source_directories": len(removable),
            "dropped_target": dropped_target, "save": retained <= limit,
            "evicted_packages": evictions, "snapshots": snapshots}


def record(name, elapsed, *, cache=None, details=None):
    root = Path(os.environ["GITHUB_WORKSPACE"])
    sys.path.insert(0, str(root / "scripts/ci"))
    import metrics
    directory = Path(os.environ["RUNNER_TEMP"]) / "ci-metrics"
    directory.mkdir(parents=True, exist_ok=True)
    value = {"schema_version": 1, "name": name, "context": metrics.runner_context(),
             "elapsed_seconds": round(elapsed, 6), "exit_code": 0,
             "cache": metrics.cache_measurement(cache), "details": details}
    (directory / f"{name}.json").write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def prepare():
    profile = os.environ["CACHE_PROFILE"]
    if profile not in PROFILES:
        raise ValueError("unsupported cache profile")
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    cargo = temp / "gitturtle-cargo"
    cargo.mkdir(exist_ok=True)
    if cargo.is_symlink() or (root / "target").is_symlink():
        raise ValueError("symlinked cache roots refused")
    identity = digest(json.dumps({"profile": profile, "source": source_identity(root),
                                 "native": native_identity(os.environ["RUNNER_OS"])}, sort_keys=True).encode())
    output_file("GITHUB_ENV", {"CI_RUST_CACHE_KEY": f"{profile}-{identity}",
                              "CI_RUST_CACHE_PROFILE": profile,
                              "CI_RUST_CACHE_LOCK": digest((root / "Cargo.lock").read_bytes()),
                              "CARGO_HOME": cargo, "CARGO_TARGET_DIR": root / "target"})


def restored():
    root, temp, paths = roots()
    started = float((temp / "gitturtle-cache-start").read_text())
    hit = os.environ.get("CACHE_HIT") == "true" and os.environ.get("CACHE_OUTCOME") == "success"
    # Upstream reports false for misses, partial compatibility matches and
    # download/extraction errors. Discard all such payloads, including partially
    # extracted archives, before Cargo is allowed to use them.
    if not hit:
        clear(paths)
    if digest((root / "Cargo.lock").read_bytes()) != os.environ["CI_RUST_CACHE_LOCK"]:
        raise ValueError("cache metadata unexpectedly changed the locked dependencies")
    elapsed = time.monotonic() - started
    record("rust-cache-restore", elapsed, cache={"hit": hit, "restore_seconds": elapsed},
           details={"key_prefix": os.environ["CI_RUST_CACHE_KEY"],
                    "boundary": "restore action plus dispatch and recovery; not transfer-only"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "start", "restored", "bound"))
    args = parser.parse_args()
    if args.phase == "prepare":
        prepare()
    elif args.phase == "start":
        (Path(os.environ["RUNNER_TEMP"]) / "gitturtle-cache-start").write_text(str(time.monotonic()))
    elif args.phase == "restored":
        restored()
    else:
        start = time.monotonic()
        root, _, paths = roots()
        if os.environ["CACHE_PROFILE"] != os.environ["CI_RUST_CACHE_PROFILE"]:
            raise ValueError("finish must match its setup profile")
        snapshots = []
        try:
            result = bound_payload(root, paths, os.environ["CACHE_PROFILE"], snapshots=snapshots)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            # Cache failure cannot turn already-passed validation into a failure;
            # retain a fixed diagnostic without leaking Cargo output or paths.
            result = {"save": False, "reason": "cache budget preparation unavailable", "snapshots": snapshots}
        output_file("GITHUB_OUTPUT", {"save": str(result["save"]).lower()})
        record("rust-cache-budget", time.monotonic() - start, details=result)
        print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
