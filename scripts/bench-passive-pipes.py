#!/usr/bin/env python3
"""Compare passive core reads from two source snapshots in a disposable fixture."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import time

HARNESS = r'''
use gitturtle_core::{GitRepository, HistoryCancellation, HistoryScope};
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let repo = GitRepository::open(std::env::args_os().nth(1).unwrap())?;
    let commits = repo.history(100)?;
    let oid = repo.changes(&commits[0].oid, 0)?[0].new_oid.clone().unwrap();
    let cancel = HistoryCancellation::default();
    for round in 0..43 {
        let start = Instant::now();
        for _ in 0..100 { assert_eq!(repo.blob(&oid)?.len(), 8192); }
        let blobs = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        assert_eq!(repo.changes(&commits[round % commits.len()].oid, 0)?.len(), 1);
        let changes = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let mut traversal = repo.history_traversal(&HistoryScope::FromCommit(commits[0].oid.clone()), &cancel)?;
        assert_eq!(traversal.next_page(500, &cancel)?.commits.len(), 100);
        let history = start.elapsed().as_secs_f64() * 1000.0;
        if round >= 3 { println!("{blobs},{changes},{history}"); }
    }
    Ok(())
}
'''


def run(args, **kwargs):
    return subprocess.check_output(args, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    baseline = run(["git", "rev-parse", args.baseline], cwd=repo).decode().strip()
    record = {
        "baseline": baseline,
        "platform": platform.platform(),
        "rust": run(["rustc", "--version"]).decode().strip(),
        "git": run(["git", "--version"]).decode().strip(),
        "harness": HARNESS,
        "script_sha256": digest(Path(__file__)),
        "conditions": "Release thin LTO, 8 codegen units; 100 linear commits, one 8192-byte blob each; same disposable fixture; OS cache not flushed; three warmup rounds and 40 measured rounds per attempt. Other desktop and development work uncontrolled. No native UI, image decoding, GPU, or RSS measurement.",
        "units": "milliseconds; blob_100 measures 100 sequential requests",
        "attempts": [],
        "sources": {},
    }
    with tempfile.TemporaryDirectory(prefix="gitturtle-passive-bench-") as temporary:
        root = Path(temporary)
        fixture = root / "fixture"
        fixture.mkdir()
        env = os.environ.copy()
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                   GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                   GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid")
        run(["git", "init", "-q", str(fixture)], env=env)
        stream = bytearray()
        for index in range(100):
            body = (f"{index:04d}\n".encode() * 1639)[:8192]
            stream.extend(f"blob\nmark :{index * 2 + 1}\ndata {len(body)}\n".encode() + body + b"\n")
            stream.extend(f"commit refs/heads/main\nmark :{index * 2 + 2}\ncommitter Fixture <fixture@example.invalid> {1700000000 + index} +0000\ndata 8\nfixture\n".encode())
            if index:
                stream.extend(f"from :{index * 2}\n".encode())
            stream.extend(f"M 100644 :{index * 2 + 1} file.txt\n\n".encode())
        subprocess.run(["git", "-C", str(fixture), "fast-import", "--quiet"], input=stream, env=env, check=True)
        run(["git", "-C", str(fixture), "symbolic-ref", "HEAD", "refs/heads/main"], env=env)
        record["fixture_tip"] = run(["git", "-C", str(fixture), "rev-parse", "HEAD"], env=env).decode().strip()
        executables = {}
        for variant in ["baseline", "corrected"]:
            source = root / variant
            source.mkdir()
            if variant == "baseline":
                names = run(["git", "ls-tree", "-r", "--name-only", baseline, "crates/git-core"], cwd=repo).decode().splitlines()
                for name in names:
                    target = source / Path(name).relative_to("crates/git-core")
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(run(["git", "show", f"{baseline}:{name}"], cwd=repo))
            else:
                shutil.copytree(repo / "crates/git-core", source, dirs_exist_ok=True)
            record["sources"][variant] = {
                str(path.relative_to(source)): digest(path)
                for path in sorted(source.rglob("*")) if path.is_file()
            }
            manifest = source / "Cargo.toml"
            manifest.write_text(manifest.read_text() + '\n[workspace]\n[profile.release]\nlto = "thin"\ncodegen-units = 8\nstrip = "debuginfo"\n')
            shutil.copyfile(repo / "Cargo.lock", source / "Cargo.lock")
            (source / "examples/pipe_audit.rs").write_text(HARNESS)
            subprocess.run(["cargo", "build", "--offline", "--release", "--manifest-path", str(manifest), "--example", "pipe_audit", "--target-dir", str(root / "target")], check=True)
            # A standalone manifest prunes unrelated workspace packages. Record
            # its resolved lock so the build's exact dependencies remain known.
            record["sources"][variant]["benchmark-lock-sha256"] = digest(source / "Cargo.lock")
            executable = root / f"{variant}-pipe-audit"
            shutil.copyfile(root / "target/release/examples/pipe_audit", executable)
            executable.chmod(0o700)
            executables[variant] = executable
            record["sources"][variant]["executable-sha256"] = digest(executable)
        for variant in ["baseline", "corrected", "corrected", "baseline"]:
            started = time.time()
            output = run([str(executables[variant]), str(fixture)], env=env).decode()
            rows = [[float(value) for value in line.split(",")] for line in output.splitlines()]
            samples = {key: [row[i] for row in rows] for i, key in enumerate(["blob_100", "changes", "history_100"])}
            record["attempts"].append({"variant": variant, "started_unix": started, "load_average": os.getloadavg(), "samples": samples})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(record, indent=2) + "\n")


if __name__ == "__main__":
    main()
