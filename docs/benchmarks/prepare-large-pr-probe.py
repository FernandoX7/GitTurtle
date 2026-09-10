#!/usr/bin/env python3
"""Prepare an isolated, offline GitHub workload probe without compiling it."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    base = subprocess.check_output(["git", "rev-parse", args.base], cwd=repo).decode().strip()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    original = {}

    def source(name):
        data = subprocess.check_output(["git", "show", f"{base}:{name}"], cwd=repo)
        original[name] = digest(data)
        return data

    def write(name, data):
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data if isinstance(data, bytes) else data.encode())

    for name in subprocess.check_output(["git", "ls-tree", "-r", "--name-only", base, "crates/git-core"], cwd=repo).decode().splitlines():
        write("git-core/" + str(Path(name).relative_to("crates/git-core")), source(name))
    for name in ["github.rs", "github/review.rs", "github/drafts.rs", "github/transport.rs"]:
        write("src/" + name, source("crates/app/src/" + name))
    preferences = source("crates/app/src/preferences.rs").decode()
    begin = preferences.index("struct PendingFile(PathBuf);")
    end = preferences.index("#[cfg(test)]\nmod tests {", begin)
    persistence = preferences[begin:end]
    write("src/preferences.rs", """use anyhow::{Context, Result, ensure};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub(super) fn settings_path() -> Result<PathBuf> {
    anyhow::bail!("Probe cannot access genuine application state; use explicit temporary paths")
}
""" + persistence)
    write("src/lib.rs", "#![allow(dead_code)]\nmod github;\nmod preferences;\n")
    harness = Path(__file__).with_name("large-pr-probe.rs").read_bytes()
    draft_file = output / "src/github/drafts.rs"
    draft_file.write_bytes(draft_file.read_bytes() + b"\n#[cfg(test)]\nmod large_pr_probe {\n" + harness + b"\n}\n")
    lock_bytes = source("Cargo.lock")
    baseline_lock = tomllib.loads(lock_bytes.decode())

    def version(name):
        matches = [package["version"] for package in baseline_lock["package"] if package["name"] == name]
        if len(matches) != 1:
            raise RuntimeError(f"Expected a unique locked version of {name}")
        return matches[0]

    manifest = """[package]
name = "gitturtle-large-pr-probe"
version = "0.0.0"
edition = "2024"

[workspace]

[dependencies]
gitturtle-core = { path = "git-core" }
"""
    for name in ["anyhow", "serde_json", "tempfile", "libc"]:
        manifest += f'{name} = "={version(name)}"\n'
    manifest += f'serde = {{ version = "={version("serde")}", features = ["derive"] }}\n'
    manifest += f'\n[target.\'cfg(target_os = "macos")\'.dependencies]\nsecurity-framework = "={version("security-framework")}"\n'
    manifest += '\n[profile.release]\nlto = "thin"\ncodegen-units = 8\nstrip = "debuginfo"\n'
    write("Cargo.toml", manifest)
    write("Cargo.lock", lock_bytes)
    # Cargo metadata only resolves/prunes the copied lock. It does not compile
    # or run a build script, probe, transport, application, or fixture.
    subprocess.run(["cargo", "metadata", "--offline", "--format-version", "1", "--manifest-path", str(output / "Cargo.toml")], check=True, stdout=subprocess.DEVNULL)
    resolved = tomllib.loads((output / "Cargo.lock").read_text())
    baseline_packages = {(package["name"], package["version"], package.get("source")): package.get("checksum") for package in baseline_lock["package"]}
    for package in resolved["package"]:
        if package.get("source"):
            key = (package["name"], package["version"], package["source"])
            if key not in baseline_packages or package.get("checksum") != baseline_packages[key]:
                raise RuntimeError(f"Dependency differs from baseline: {key}")
    record = {
        "base": base,
        "preparer_sha256": digest(Path(__file__).read_bytes()),
        "harness_sha256": digest(harness),
        "original_source_sha256": original,
        "persistence_extracted_sha256": digest(persistence.encode()),
        "generated_files_sha256": {str(path.relative_to(output)): digest(path.read_bytes()) for path in sorted(output.rglob("*")) if path.is_file()},
        "notes": "GitHub modules/core copied verbatim from base, except appended test-only module. Exact PendingFile/read_store/read_store_contents/atomic_write source extracted. settings_path deliberately errors. Production Transport implementation is compiled but never called; probe uses synthetic GET-only transport. All registry versions/checksums match baseline lock. Metadata resolution ran; compiler/test has not run.",
        "test_command": ["cargo", "test", "--release", "--locked", "--offline", "--manifest-path", str(output / "Cargo.toml"), "large_offline_pr_probe", "--", "--nocapture", "--test-threads=1"],
    }
    write("provenance.json", json.dumps(record, indent=2) + "\n")
    print(json.dumps({"prepared": str(output), "base": base, "harness_sha256": record["harness_sha256"], "registry_packages": sum(bool(package.get("source")) for package in resolved["package"]), "compiled": False}))


if __name__ == "__main__":
    main()
