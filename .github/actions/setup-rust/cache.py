#!/usr/bin/env python3
"""Lifecycle of an optional, bounded cache on fresh GitHub-hosted Rust runners."""
import argparse
import hashlib
import json
import os
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


def entries(paths):
    """Conservative payload accounting: logical bytes + 4 KiB per entry.

    Counts aliases separately, and never follows directory/file symlinks. This is
    an upper bound before upstream dependency cleanup, not a compressed archive.
    """
    result = []
    for path in paths:
        if not path.exists():
            continue
        for directory, dirs, files in os.walk(path, followlinks=False):
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


def bound_payload(root, paths, profile, *, execute=run):
    started = time.monotonic()
    limit = SERIAL_LIMIT if profile == "debug-release" else SINGLE_PROFILE_LIMIT
    metadata = json.loads(execute(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1"], cwd=root, timeout=30))
    packages = metadata["packages"]
    local = [package["id"] for package in packages if package.get("source") is None]
    if local:
        execute(["cargo", "clean", "--locked", *[part for package_id in local for part in ("--package", package_id)]], cwd=root, timeout=30)
    measured = entries(paths)
    before = sum(size for _, size in measured)
    removed = 0
    ordered = largest_packages([p for p in packages if p.get("source") is not None],
                               [(p, size) for p, size in measured if p.is_relative_to(paths[0])])
    for package_id in ordered[:MAX_CLEAN_PACKAGES]:
        if sum(size for _, size in measured) <= limit or time.monotonic() - started >= BUDGET_SECONDS:
            break
        execute(["cargo", "clean", "--locked", "--package", package_id], cwd=root, timeout=30)
        removed += 1
        measured = entries(paths)
    dropped_target = sum(size for _, size in measured) > limit
    if dropped_target:
        clear(paths[:1])
        measured = entries(paths)
    retained = sum(size for _, size in measured)
    return {"limit_bytes": limit, "before_bytes": before, "retained_bytes": retained,
            "removed_dependency_packages": removed, "dropped_target": dropped_target,
            "save": retained <= limit}


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
        try:
            result = bound_payload(root, paths, os.environ["CACHE_PROFILE"])
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            # Cache failure cannot turn already-passed validation into a failure;
            # retain a fixed diagnostic without leaking Cargo output or paths.
            result = {"save": False, "reason": "cache budget preparation unavailable"}
        output_file("GITHUB_OUTPUT", {"save": str(result["save"]).lower()})
        record("rust-cache-budget", time.monotonic() - start, details=result)
        print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
