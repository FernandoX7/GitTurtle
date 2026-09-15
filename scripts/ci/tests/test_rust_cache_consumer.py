"""Opt-in pinned Cargo consumer proof using disposable registry and Git packages."""
import functools
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import threading
import tomllib
import unittest
from unittest.mock import patch

from test_rust_cache import ROOT, cache


class QuietRegistry(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass


@unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                     "Set GITTURTLE_CACHE_CARGO_QA=1 for the pinned Cargo consumer fixture")
class CargoConsumerFixture(unittest.TestCase):
    def test_pinned_registry_native_and_git_dependencies_stay_fresh_and_eviction_recovers(self):
        # The server and Git origin are disposable local fixtures, with no public
        # service, user Cargo home, or workspace output used by this test.
        (ROOT / ".local").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="cache-consumer-", dir=ROOT / ".local") as directory:
            base = Path(directory)
            root, cargo, registry = (base / name for name in ("repo", "cargo", "registry"))
            for path in (root, cargo, registry):
                path.mkdir()
            target = root / "target"
            env = {**os.environ, "CARGO_HOME": str(cargo), "CARGO_TARGET_DIR": str(target),
                   "CARGO_INCREMENTAL": "0", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}
            toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
            cargo_command = ["cargo", f"+{toolchain}"]

            def write(path, value):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(value)

            def run(command, *, cwd=root, timeout=30):
                if command[0] == "cargo":
                    command = [*cargo_command, *command[1:]]
                result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, timeout=timeout)
                self.assertEqual(result.returncode, 0, (command, result.stderr.decode(errors="replace")))
                return result

            self.assertTrue(run(["cargo", "--version"]).stdout.decode().startswith(f"cargo {toolchain} "))
            handler = functools.partial(QuietRegistry, directory=str(registry))
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            self.addCleanup(server.server_close)
            self.addCleanup(thread.join)
            self.addCleanup(server.shutdown)
            url = f"http://127.0.0.1:{server.server_port}"
            write(registry / "config.json", json.dumps({"dl": f"{url}/crates/{{crate}}/{{version}}/download"}))

            def publish_fixture(name, files, extra=""):
                manifest = f'[package]\nname="{name}"\nversion="1.0.0"\nedition="2021"\n{extra}'
                archive = io.BytesIO()
                with tarfile.open(fileobj=archive, mode="w:gz") as tar:
                    for relative, contents in {"Cargo.toml": manifest, **files}.items():
                        data = contents.encode()
                        info = tarfile.TarInfo(f"{name}-1.0.0/{relative}")
                        info.size, info.mtime, info.mode = len(data), 1_700_000_000, 0o644
                        tar.addfile(info, io.BytesIO(data))
                payload = archive.getvalue()
                path = registry / "crates" / name / "1.0.0/download"
                path.parent.mkdir(parents=True)
                path.write_bytes(payload)
                write(registry / name[:2] / name[2:4] / name,
                      json.dumps({"name": name, "vers": "1.0.0", "deps": [],
                                  "cksum": hashlib.sha256(payload).hexdigest(), "features": {}, "yanked": False}) + "\n")

            publish_fixture("cache-pure", {"src/lib.rs": "pub fn answer() -> u32 { 1 }\n"})
            publish_fixture("cache-native-sys", {
                "src/lib.rs": 'extern "C" { fn fixture_native() -> u32; } pub fn answer() -> u32 { unsafe { fixture_native() } }\n',
                "native.c": "unsigned int fixture_native(void) { return 42; }\n",
                "build.rs": r'''use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=native.c");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("native.o");
    assert!(Command::new("cc").args(["-c", "native.c", "-o"]).arg(&object).status().unwrap().success());
    assert!(Command::new("ar").arg("crs").arg(out.join("libfixture_native.a")).arg(&object).status().unwrap().success());
    fs::write(out.join("generated.rs"), "pub const GENERATED:u32=42;").unwrap();
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=fixture_native");
}
''',
            }, extra='links="fixture_native"\n')
            git = base / "git-origin"
            git.mkdir()
            write(git / "Cargo.toml", '[package]\nname="cache-git"\nversion="1.0.0"\nedition="2021"\n')
            write(git / "src/lib.rs", 'include!(concat!(env!("OUT_DIR"),"/answer.rs"));\npub fn answer()->u32{ANSWER}\n')
            write(git / "build.rs", 'fn main(){println!("cargo:rerun-if-changed=build.rs");std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("answer.rs"),"const ANSWER:u32=2;").unwrap();}\n')
            run(["git", "init", "-q"], cwd=git)
            run(["git", "add", "Cargo.toml", "build.rs", "src/lib.rs"], cwd=git)
            run(["git", "-c", "user.name=Cache Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], cwd=git)
            revision = run(["git", "rev-parse", "HEAD"], cwd=git).stdout.decode().strip()
            write(root / ".cargo/config.toml", f'[registries.fixture]\nindex="sparse+{url}/"\n')
            write(root / "Cargo.toml", '[workspace]\n[package]\nname="cache-consumer"\nversion="1.0.0"\nedition="2021"\n'
                  '[dependencies]\ncache-pure={version="=1.0.0",registry="fixture"}\n'
                  'cache-native-sys={version="=1.0.0",registry="fixture"}\n'
                  f'cache-git={{git="{git.as_uri()}",rev="{revision}"}}\n')
            write(root / "src/lib.rs", '#[test] fn consumer(){assert_eq!(cache_native_sys::answer()+cache_pure::answer()+cache_git::answer(),45);}\n')
            run(["cargo", "generate-lockfile"])
            run(["cargo", "fetch", "--locked"])
            locked = (root / "Cargo.lock").read_bytes()
            command = ["cargo", "test", "--locked", "--offline", "-vv"]
            first = run(command)
            self.assertIn(b"1 passed", first.stdout)
            metadata = json.loads(run(["cargo", "metadata", "--locked", "--offline", "--all-features", "--format-version", "1"]).stdout)
            packages = {package["name"]: package for package in metadata["packages"]}
            native_source = Path(packages["cache-native-sys"]["manifest_path"]).parent
            pure_source = Path(packages["cache-pure"]["manifest_path"]).parent
            git_source = Path(packages["cache-git"]["manifest_path"]).parent
            self.assertTrue(packages["cache-native-sys"]["source"].startswith("sparse+"), packages["cache-native-sys"]["source"])
            self.assertTrue(packages["cache-git"]["source"].startswith("git+"))
            sys_timestamp = (native_source / "native.c").stat().st_mtime_ns
            outputs = list(target.glob("debug/build/cache-native-sys-*/out/*")) + list(target.glob("debug/build/cache-git-*/out/*"))
            self.assertTrue(any(path.suffix == ".a" for path in outputs))
            self.assertTrue(any(path.name == "answer.rs" for path in outputs))
            retained = {path: (path.read_bytes(), path.stat().st_mtime_ns) for path in outputs}
            paths = (target, cargo / "registry", cargo / "git")

            def execute(command, **kwargs):
                return run(command, **kwargs).stdout

            result = cache.bound_payload(root, paths, "debug", execute=execute)
            self.assertTrue(result["save"])
            self.assertFalse(result["dropped_target"])
            self.assertEqual(result["removed_dependency_packages"], 0)
            self.assertFalse(pure_source.exists())
            self.assertTrue(native_source.exists())
            self.assertTrue(git_source.exists())
            self.assertTrue(list((cargo / "git/db").iterdir()))
            self.assertEqual((native_source / "native.c").stat().st_mtime_ns, sys_timestamp)
            # Actual pinned Cargo metadata recreates the pure-Rust source before
            # pinned rust-cache's post cleanup removes it again. The post action
            # also prunes targets/index/archive/Git; those further reductions are
            # not simulated or represented as archive-service evidence here.
            run(["cargo", "metadata", "--locked", "--offline", "--all-features", "--format-version", "1"])
            self.assertTrue(pure_source.exists())
            cache.clear(cache.removable_sources(metadata["packages"], paths, cache.entries(paths)))
            self.assertFalse(pure_source.exists())
            self.assertEqual(sum(size for _, size in cache.entries(paths)), result["retained_bytes"])
            # Round-trip the eligible files with their timestamps through a real
            # archive. Fixed roots were validated above; no untrusted archive is
            # accepted, and this is not a GitHub service/Swatinem archive claim.
            archive_path = base / "fixture-cache.tar"
            with tarfile.open(archive_path, "w") as archive:
                for path in paths:
                    archive.add(path, arcname=path.relative_to(base))
            cache.clear(paths)
            with tarfile.open(archive_path) as archive:
                archive.extractall(base, filter="data")
            for path, (data, mtime) in retained.items():
                self.assertEqual(path.read_bytes(), data)
                # Python's tar mtime uses float seconds; allow only its sub-µs
                # conversion loss, then prove the consumer leaves it unchanged.
                self.assertLessEqual(abs(path.stat().st_mtime_ns - mtime), 1000)
            restored_times = {path: path.stat().st_mtime_ns for path in retained}
            second = run(command)
            self.assertIn(b"1 passed", second.stdout)
            self.assertIn(b"Compiling cache-consumer", second.stderr)
            for name in ("cache-pure", "cache-native-sys", "cache-git"):
                self.assertIn(f"Fresh {name} v1.0.0".encode(), second.stderr)
                self.assertNotIn(f"Compiling {name} ".encode(), second.stderr)
            self.assertEqual((native_source / "native.c").stat().st_mtime_ns, sys_timestamp)
            for path, (data, mtime) in retained.items():
                self.assertEqual((path.read_bytes(), path.stat().st_mtime_ns), (data, restored_times[path]))
            # Force exactly the native package to be the largest cleanup choice.
            # Cargo must remove its fingerprint and generated outputs together;
            # the next real consumer then regenerates and links successfully.
            fingerprints = list(target.glob("debug/.fingerprint/cache-native-sys-*"))
            self.assertTrue(fingerprints)
            padding = target / "debug/build/cache-native-sys-padding/out/padding"
            padding.parent.mkdir(parents=True)
            padding.write_bytes(b"p" * 4 * 1024 * 1024)
            with patch.object(cache, "SINGLE_PROFILE_LIMIT", result["retained_bytes"] + 2 * 1024 * 1024):
                evicted = cache.bound_payload(root, paths, "debug", execute=execute)
            self.assertTrue(evicted["save"])
            self.assertFalse(evicted["dropped_target"])
            self.assertEqual(evicted["removed_dependency_packages"], 1)
            self.assertFalse(any(path.exists() for path in retained if "cache-native-sys" in str(path)))
            self.assertFalse(any(path.exists() for path in fingerprints))
            self.assertEqual(evicted["evicted_packages"][0]["name"], "cache-native-sys")
            self.assertEqual(evicted["evicted_packages"][0]["version"], "1.0.0")
            third = run(command)
            self.assertIn(b"1 passed", third.stdout)
            self.assertIn(b"Compiling cache-native-sys v1.0.0", third.stderr)
            self.assertIn(b"Fresh cache-pure v1.0.0", third.stderr)
            self.assertIn(b"Fresh cache-git v1.0.0", third.stderr)
            self.assertEqual((root / "Cargo.lock").read_bytes(), locked)
