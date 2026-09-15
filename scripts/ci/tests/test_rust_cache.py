"""Behavioral tests for cache identity, bounded cleanup and miss recovery."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("rust_cache", ROOT / ".github/actions/setup-rust/cache.py")
cache = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cache)


class CacheFixture(unittest.TestCase):
    def setUp(self):
        (ROOT / ".local").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="cache-test-", dir=ROOT / ".local")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "repo"
        self.root.mkdir()
        self.runner = self.base / "runner"
        self.runner.mkdir()
        self.cargo = self.runner / "gitturtle-cargo"
        self.cargo.mkdir()
        self.paths = (self.root / "target", self.cargo / "registry", self.cargo / "git")
        self.env = {"GITHUB_WORKSPACE": str(self.root), "RUNNER_TEMP": str(self.runner),
                    "CARGO_HOME": str(self.cargo), "CARGO_TARGET_DIR": str(self.paths[0]),
                    "GITHUB_ENV": str(self.runner / "env"), "GITHUB_OUTPUT": str(self.runner / "output")}
        self.patch = patch.dict(os.environ, self.env)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def file(self, path, content=b"x"):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def init_git(self):
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates/app/src/main.rs",
                     "crates/app/build.rs", "crates/app/Cargo.toml", "vendor/tool/src/lib.rs",
                     "vendor/tool/native/source.c", "vendor/tool/build.rs", ".github/actions/setup-rust/action.yml"):
            self.file(self.root / name, name.encode())
        subprocess.run(["git", "-C", str(self.root), "add", "--all"], check=True)

    def test_source_only_edits_reuse_but_real_build_inputs_invalidate(self):
        self.init_git()
        original = cache.source_identity(self.root)
        self.file(self.root / "crates/app/src/main.rs", b"changed product source")
        self.assertEqual(original, cache.source_identity(self.root))
        for path in ("Cargo.lock", "Cargo.toml", "crates/app/Cargo.toml", "rust-toolchain.toml",
                     "crates/app/build.rs", "vendor/tool/src/lib.rs", "vendor/tool/native/source.c",
                     "vendor/tool/build.rs", ".github/actions/setup-rust/action.yml"):
            with self.subTest(path=path):
                before = cache.source_identity(self.root)
                self.file(self.root / path, b"changed build input")
                self.assertNotEqual(before, cache.source_identity(self.root))

    def test_deleted_build_input_refuses_cache_identity(self):
        self.init_git()
        (self.root / "vendor/tool/build.rs").unlink()
        with self.assertRaises(FileNotFoundError):
            cache.source_identity(self.root)

    def test_native_versions_are_part_of_compatibility(self):
        for platform in ("Linux", "macOS"):
            with self.subTest(platform=platform):
                with patch.object(cache, "run", return_value=b"old native inputs"):
                    old = cache.native_identity(platform)
                with patch.object(cache, "run", return_value=b"updated native inputs"):
                    self.assertNotEqual(old, cache.native_identity(platform))
        with self.assertRaises(ValueError):
            cache.native_identity("Windows")

    def test_profiles_have_distinct_identity_and_private_paths(self):
        self.init_git()
        identities = []
        for profile in sorted(cache.PROFILES):
            with patch.dict(os.environ, {"CACHE_PROFILE": profile, "RUNNER_OS": "Linux"}), patch.object(cache, "native_identity", return_value="native"):
                cache.prepare()
            values = dict(line.split("=", 1) for line in Path(self.env["GITHUB_ENV"]).read_text().splitlines())
            identities.append(values["CI_RUST_CACHE_KEY"])
            self.assertEqual(values["CARGO_HOME"], str(self.cargo))
            self.assertEqual(values["CARGO_TARGET_DIR"], str(self.paths[0]))
        self.assertEqual(len(set(identities)), len(cache.PROFILES))

    def test_unexpected_target_or_cargo_home_refuses_cleanup(self):
        for key in ("CARGO_HOME", "CARGO_TARGET_DIR"):
            with self.subTest(key=key), patch.dict(os.environ, {key: str(self.base)}):
                with self.assertRaises(ValueError):
                    cache.roots()

    def test_payload_never_follows_symlinks(self):
        external = self.file(self.base / "external", b"preserve")
        self.paths[0].mkdir()
        (self.paths[0] / "alias").symlink_to(external)
        with self.assertRaises(ValueError):
            cache.entries(self.paths)
        cache.clear(self.paths)
        self.assertEqual(external.read_bytes(), b"preserve")

    def test_entry_budget_and_hardlink_accounting_are_conservative(self):
        first = self.file(self.paths[0] / "first", b"1234")
        second = self.paths[0] / "second"
        os.link(first, second)
        self.assertEqual(sum(size for _, size in cache.entries(self.paths)), 2 * (4096 + 4))
        with patch.object(cache, "MAX_FILES", 1):
            with self.assertRaises(ValueError):
                cache.entries(self.paths)

    def restore(self, hit="true", outcome="success"):
        self.file(self.root / "Cargo.lock", b"locked")
        self.file(self.runner / "gitturtle-cache-start", b"1")
        for path in self.paths:
            self.file(path / "partial", b"cached")
        with patch.dict(os.environ, {"CACHE_HIT": hit, "CACHE_OUTCOME": outcome,
                                    "CI_RUST_CACHE_LOCK": cache.digest(b"locked"),
                                    "CI_RUST_CACHE_KEY": "fixture-key"}), patch.object(cache, "record") as record:
            cache.restored()
        return record.call_args

    def test_exact_successful_restore_keeps_payload(self):
        args = self.restore()
        self.assertTrue(all(path.exists() for path in self.paths))
        self.assertTrue(args.kwargs["cache"]["hit"])
        self.assertNotIn("size_bytes", args.kwargs["cache"])
        self.assertNotIn("save_seconds", args.kwargs["cache"])

    def test_miss_fallback_error_or_missing_result_discards_partial_payload(self):
        for hit, outcome in (("false", "success"), ("", "failure"), ("true", "failure"), ("", "success")):
            with self.subTest(hit=hit, outcome=outcome):
                args = self.restore(hit, outcome)
                self.assertFalse(any(path.exists() for path in self.paths))
                self.assertFalse(args.kwargs["cache"]["hit"])

    def test_bounds_clean_whole_packages_then_keep_useful_dependencies(self):
        local = self.file(self.paths[0] / "debug/deps/libapp-abcd.rlib", b"L" * 100)
        large = self.file(self.paths[0] / "debug/deps/liblarge-abcd.rlib", b"G" * 200)
        small = self.file(self.paths[0] / "debug/deps/libsmall-abcd.rlib", b"S")
        calls = []
        packages = [{"name": name, "id": name, "source": None if name == "app" else "registry", "targets": [{"name": name}]} for name in ("app", "large", "small")]

        def execute(command, **_):
            calls.append(command)
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            {"app": local, "large": large, "small": small}[command[-1]].unlink()
            return b""

        # Two directories plus one small dependency fit after Cargo removes app/large.
        with patch.object(cache, "SERIAL_LIMIT", 3 * 4096 + 5):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=execute)
        self.assertTrue(result["save"])
        self.assertFalse(result["dropped_target"])
        self.assertFalse(local.exists())
        self.assertFalse(large.exists())
        self.assertTrue(small.exists())
        self.assertEqual(result["removed_dependency_packages"], 1)
        self.assertTrue(all("--locked" in command for command in calls))

    def test_oversized_target_is_discarded_without_partial_native_outputs(self):
        self.file(self.paths[0] / "debug/build/sys-hash/out/include.h", b"native")
        download = self.file(self.paths[1] / "dependency.crate", b"download")
        with patch.object(cache, "SERIAL_LIMIT", 8192):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=lambda *a, **k: b'{"packages": []}')
        self.assertTrue(result["save"])
        self.assertTrue(result["dropped_target"])
        self.assertFalse(self.paths[0].exists())
        self.assertTrue(download.exists())

    def test_oversized_downloads_prevent_save_without_deleting_credentials(self):
        self.file(self.paths[1] / "dependency.crate", b"large")
        credential = self.file(self.cargo / "credentials.toml", b"not in cache")
        with patch.object(cache, "SERIAL_LIMIT", 10):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=lambda *a, **k: b'{"packages": []}')
        self.assertFalse(result["save"])
        self.assertEqual(credential.read_bytes(), b"not in cache")

    def test_projection_prunes_only_disposable_sources_after_all_cargo_commands(self):
        library = self.file(self.paths[0] / "debug/deps/libkeep-abcd.rlib", b"compiled")
        generated = self.file(self.paths[0] / "debug/build/keep-sys-abcd/out/native.a", b"native")
        fingerprint = self.file(self.paths[0] / "debug/.fingerprint/keep-sys-abcd/lib-keep", b"fingerprint")
        pure = self.file(self.paths[1] / "src/registry/pure-1.0.0/src/lib.rs", b"R" * 200_000)
        outdated = self.file(self.paths[1] / "src/registry/old-sys-0.1.0/native.c", b"old")
        native = self.file(self.paths[1] / "src/registry/keep-sys-1.0.0/native.c", b"timestamp-sensitive")
        checkout = self.file(self.paths[2] / "checkouts/dep/ref/src/lib.rs", b"git source")
        database = self.file(self.paths[2] / "db/dep/objects/pack/pack.data", b"git database")
        timestamp = native.stat().st_mtime_ns
        packages = [{"name": "keep-sys", "version": "1.0.0", "id": "keep", "source": "registry"},
                    {"name": "app", "id": "app", "source": None}]
        calls = []

        def execute(command, **_):
            calls.append(command)
            self.assertTrue(pure.exists(), "Cargo commands must run before source pruning")
            return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

        with patch.object(cache, "SINGLE_PROFILE_LIMIT", 180_000):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertTrue(result["save"])
        self.assertFalse(result["dropped_target"])
        self.assertEqual(result["removed_dependency_packages"], 0)
        self.assertEqual(result["removed_source_directories"], 2)
        self.assertEqual(len(calls), 2)
        self.assertFalse(pure.exists())
        self.assertFalse(outdated.exists())
        self.assertEqual(native.stat().st_mtime_ns, timestamp)
        for path in (library, generated, fingerprint, checkout, database):
            self.assertTrue(path.exists())
        initial, _, final = result["snapshots"]
        self.assertGreater(initial["bytes"], result["limit_bytes"])
        self.assertEqual(initial["projected_bytes"], final["bytes"])
        self.assertEqual(final["projected_bytes"], final["bytes"])
        self.assertEqual(final["compiled"]["libraries"]["files"], 1)
        self.assertEqual(final["compiled"]["native_generated"]["files"], 1)
        self.assertEqual(final["compiled"]["fingerprints"]["files"], 1)
        self.assertEqual(final["bytes"], final["logical_bytes"] + final["entry_overhead_bytes"])
        self.assertEqual(final["subtrees"]["registry_src_removable"]["entries"], 0)
        self.assertNotIn(str(self.base), json.dumps(result))

    def test_excluded_sources_still_enforce_type_and_entry_limits(self):
        source = self.file(self.paths[1] / "src/registry/pure-1.0.0/lib.rs", b"source")
        execute = lambda *a, **k: b'{"packages": []}'
        with patch.object(cache, "MAX_FILES", 2), self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertTrue(source.exists())
        source.unlink()
        source.symlink_to(self.file(self.base / "outside", b"preserved"))
        with self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        source.unlink()
        os.mkfifo(source)
        with self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual((self.base / "outside").read_bytes(), b"preserved")

    def test_accounting_refuses_expired_time_nondirectory_and_symlink_roots(self):
        source = self.file(self.paths[1] / "src/registry/pure-1.0.0/lib.rs")
        with patch.object(cache, "BUDGET_SECONDS", 0), self.assertRaises(TimeoutError):
            cache.bound_payload(self.root, self.paths, "debug", execute=lambda *a, **k: b'{"packages": []}')
        self.assertTrue(source.exists())
        self.file(self.paths[0])
        with self.assertRaises(ValueError):
            cache.entries(self.paths)
        self.paths[0].unlink()
        self.paths[0].symlink_to(self.paths[1], target_is_directory=True)
        with self.assertRaises(ValueError):
            cache.entries(self.paths)

    def test_cleanup_never_exceeds_eight_dependency_groups(self):
        packages = [{"name": f"p{i}", "id": f"p{i}", "source": "registry"} for i in range(12)]
        for package in packages:
            self.file(self.paths[0] / f"debug/deps/{package['name']}-hash.rlib")
        calls = []

        def execute(command, **_):
            calls.append(command)
            return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

        with patch.object(cache, "SINGLE_PROFILE_LIMIT", 1):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["removed_dependency_packages"], 8)
        self.assertEqual(len([command for command in calls if command[1] == "clean"]), 8)
        self.assertTrue(result["dropped_target"])

    def test_eviction_diagnostics_keep_safe_names_without_source_ids(self):
        package_id = "git+https://example.invalid/private?token=private#revision"
        packages = [{"name": "fixture", "version": "1.2.3", "id": package_id, "source": "git"}]
        artifact = self.file(self.paths[0] / "debug/deps/fixture-hash.rlib", b"artifact")

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            artifact.unlink()
            return b""

        with patch.object(cache, "SINGLE_PROFILE_LIMIT", 8192):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "version": "1.2.3"}])
        self.assertNotIn("private", json.dumps(result))
        # Unexpected metadata names are still identifiable without emitting
        # delimiters, a URL, a path or an unbounded string into public logs.
        packages[0]["version"] = "private/path" + "x" * 200
        artifact.write_bytes(b"again")
        with patch.object(cache, "SINGLE_PROFILE_LIMIT", 8192):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture"}])
        self.assertNotIn("private", json.dumps(result))

    def test_failed_budget_preserves_completed_snapshots_and_fixed_reason(self):
        self.file(self.paths[0] / "debug/deps/fixture-hash.rlib")
        snapshot = {"stage": "before_local_cleanup", "bytes": 123}

        def unavailable(*_, snapshots, **__):
            snapshots.append(snapshot)
            raise ValueError("private/path or command output must not be logged")

        with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "CI_RUST_CACHE_PROFILE": "debug"}), \
                patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", side_effect=unavailable), \
                patch.object(cache, "record") as record, patch("builtins.print"):
            cache.main()
        result = record.call_args.kwargs["details"]
        self.assertFalse(result["save"])
        self.assertEqual(result["snapshots"], [snapshot])
        self.assertEqual(result["reason"], "cache budget preparation unavailable")
        self.assertNotIn("private", json.dumps(result))
        self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), "save=false\n")

    @unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                         "Set GITTURTLE_CACHE_CARGO_QA=1 for the real Cargo recovery fixture")
    def test_real_cargo_cleanup_rebuilds_local_crate_and_native_generated_output(self):
        self.file(self.root / "Cargo.toml", b'[workspace]\n[package]\nname="cache_recovery_fixture"\nversion="0.1.0"\nedition="2021"\n')
        self.file(self.root / "build.rs", b'use std::{env,fs,path::Path}; fn main() { fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("answer.rs"), "pub const ANSWER:u32=42;").unwrap(); }')
        self.file(self.root / "src/lib.rs", b'include!(concat!(env!("OUT_DIR"),"/answer.rs")); #[test] fn generated_answer(){assert_eq!(ANSWER,42);}')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True, capture_output=True)
        command = ["cargo", "test", "--locked", "--offline"]
        first = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
        self.assertIn(b"1 passed", first.stdout)
        self.assertTrue(list(self.paths[0].glob("debug/build/*/out/answer.rs")))
        result = cache.bound_payload(self.root, self.paths, "debug-release")
        self.assertTrue(result["save"])
        self.assertFalse(list(self.paths[0].glob("debug/build/*/out/answer.rs")))
        second = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
        self.assertIn(b"1 passed", second.stdout)
        self.assertTrue(list(self.paths[0].glob("debug/build/*/out/answer.rs")))
        self.assertIn(b"Compiling cache_recovery_fixture", second.stderr)


if __name__ == "__main__":
    unittest.main()
