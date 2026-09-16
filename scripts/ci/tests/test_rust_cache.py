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


class FakeClock:
    """Monotonic time that advances only when the fixture's fake work runs."""

    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


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
        packages = [{"name": name, "id": name, "source": None if name == "app" else "registry", "targets": [{"name": name, "kind": ["lib"]}]} for name in ("app", "large", "small")]

        def execute(command, **_):
            calls.append(command)
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            {"app": local, "large": large, "small": small}[command[-1]].unlink(missing_ok=True)
            return b""

        # Two directories plus one small dependency fit after Cargo removes app/large.
        with patch.dict(cache.PROFILE_LIMITS, {"debug-release": 3 * 4096 + 5}):
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
        with patch.dict(cache.PROFILE_LIMITS, {"debug-release": 8192}):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=lambda *a, **k: b'{"packages": []}')
        self.assertTrue(result["save"])
        self.assertTrue(result["dropped_target"])
        self.assertFalse(self.paths[0].exists())
        self.assertTrue(download.exists())

    def test_oversized_downloads_prevent_save_without_deleting_credentials(self):
        self.file(self.paths[1] / "dependency.crate", b"large")
        credential = self.file(self.cargo / "credentials.toml", b"not in cache")
        with patch.dict(cache.PROFILE_LIMITS, {"debug-release": 10}):
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

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 180_000}):
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

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 1}):
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

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 8192}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "version": "1.2.3", "cleanup_completed": True}])
        self.assertNotIn("private", json.dumps(result))
        # Unexpected metadata names are still identifiable without emitting
        # delimiters, a URL, a path or an unbounded string into public logs.
        packages[0]["version"] = "private/path" + "x" * 200
        artifact.write_bytes(b"again")
        with patch.dict(cache.PROFILE_LIMITS, {"debug": 8192}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "cleanup_completed": True}])
        self.assertNotIn("private", json.dumps(result))

    def test_failed_budget_preserves_completed_snapshots_and_fixed_reason(self):
        self.file(self.paths[0] / "debug/deps/fixture-hash.rlib")
        snapshot = {"stage": "before_local_cleanup", "bytes": 123}

        def unavailable(*_, snapshots, diagnostics, **__):
            snapshots.append(snapshot)
            diagnostics.update(limit_bytes=123_456, stage="after_dependency_cleanup_1", stage_seconds=[{"stage": "after_dependency_cleanup_1", "elapsed_seconds": 0.5}],
                               evicted_packages=[{"metadata_index": 0, "name": "fixture", "cleanup_completed": True}])
            raise ValueError("private/path or command output must not be logged")

        with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "CI_RUST_CACHE_PROFILE": "debug"}), \
                patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", side_effect=unavailable), \
                patch.object(cache, "record") as record, patch("builtins.print"):
            cache.main()
        result = record.call_args.kwargs["details"]
        self.assertFalse(result["save"])
        self.assertEqual(result["snapshots"], [snapshot])
        self.assertEqual(result["reason"], "cache budget preparation unavailable")
        self.assertEqual(result["failure_stage"], "after_dependency_cleanup_1")
        self.assertEqual(result["limit_bytes"], 123_456)
        self.assertEqual(result["stage_seconds"], [{"stage": "after_dependency_cleanup_1", "elapsed_seconds": 0.5}])
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "cleanup_completed": True}])
        self.assertNotIn("private", json.dumps(result))
        self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), "save=false\n")

    def test_profile_cleanup_failure_retains_fixed_stage_and_incomplete_group(self):
        self.file(self.paths[0] / "release/deps/libfixture-hash.rlib", b"artifact")
        diagnostics = {}
        packages = [{"name": "fixture", "version": "1.0.0", "id": "private/package-id", "source": "registry", "targets": [{"name": "fixture", "kind": ["lib"]}]}]

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            raise subprocess.TimeoutExpired(command, 30, output=b"private output")

        with patch.dict(cache.PROFILE_LIMITS, {"release": 1}), self.assertRaises(subprocess.TimeoutExpired):
            cache.bound_payload(self.root, self.paths, "release", execute=execute, diagnostics=diagnostics)
        self.assertEqual(diagnostics["stage"], "dependency_cleanup_1_release")
        self.assertEqual(diagnostics["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "version": "1.0.0", "cleanup_completed": False}])
        self.assertEqual(diagnostics["stage_seconds"][-1]["stage"], "dependency_cleanup_1_release")
        self.assertTrue(all(row["elapsed_seconds"] >= 0 for row in diagnostics["stage_seconds"]))
        self.assertNotIn("private", json.dumps(diagnostics))

    def test_selection_attributes_structured_artifacts_not_colliding_names(self):
        # Test/example targets named "debug" or "build" and every build script's
        # "build_script_build" binary once matched whole subtrees. The root sits
        # below an absolute "build/repo-0" directory: components above the root,
        # entries outside it, and test binaries named after test targets never score.
        target = self.base / "build/repo-0/target"
        packages = [
            {"name": "serde_json", "id": "serde_json", "source": "registry",
             "targets": [{"name": "serde_json", "kind": ["lib"]}, {"name": "debug", "kind": ["test"]},
                         {"name": "build-script-build", "kind": ["custom-build"]}]},
            {"name": "clang-sys", "id": "clang-sys", "source": "registry",
             "targets": [{"name": "clang_sys", "kind": ["lib"]}, {"name": "build", "kind": ["test"]},
                         {"name": "lib", "kind": ["test"]}, {"name": "build-script-build", "kind": ["custom-build"]}]},
            {"name": "bulky", "id": "bulky", "source": "registry", "targets": [{"name": "bulky", "kind": ["lib"]}]},
            {"name": "repo", "id": "repo", "source": "registry", "targets": [{"name": "repo", "kind": ["lib"]}]},
        ]
        for relative, data in (("debug/deps/libbulky-5e6f.rlib", b"B" * 100_000),
                               ("debug/deps/libserde_json-1a2b.rlib", b"j" * 30_000),
                               ("debug/build/serde_json-1a2b/build_script_build-1a2b", b"s" * 1_000),
                               ("debug/deps/libclang_sys-3c4d.rlib", b"c" * 10_000),
                               ("debug/build/clang-sys-3c4d/out/bindings.rs", b"o" * 8_000),
                               ("debug/build/clang-sys-3c4d/build_script_build-3c4d", b"s" * 1_000),
                               ("debug/.fingerprint/clang-sys-3c4d/lib-clang_sys", b"f"),
                               ("debug/app-9f9f", b"a" * 500_000), ("debug/deps/app-9f9f", b"a" * 500_000),
                               ("debug/deps/debug-9a9a", b"t" * 400_000), ("debug/deps/lib-9b9b", b"t" * 400_000)):
            self.file(target / relative, data)
        self.file(self.paths[1] / "src/build/bulky-1.0.0/lib.rs", b"r" * 400_000)
        owned = {
            "bulky": {"debug/deps/libbulky-5e6f.rlib"},
            "serde_json": {"debug/deps/libserde_json-1a2b.rlib", "debug/build/serde_json-1a2b",
                           "debug/build/serde_json-1a2b/build_script_build-1a2b"},
            "clang-sys": {"debug/deps/libclang_sys-3c4d.rlib", "debug/build/clang-sys-3c4d", "debug/build/clang-sys-3c4d/out",
                          "debug/build/clang-sys-3c4d/out/bindings.rs", "debug/build/clang-sys-3c4d/build_script_build-3c4d",
                          "debug/.fingerprint/clang-sys-3c4d", "debug/.fingerprint/clang-sys-3c4d/lib-clang_sys"},
        }
        measured = cache.entries((target, self.paths[1]))
        expected = {name: sum(size for path, size in measured
                              if path.is_relative_to(target) and str(path.relative_to(target)) in members)
                    for name, members in owned.items()}
        self.assertEqual(cache.package_artifact_bytes(packages, measured, target), expected)
        self.assertEqual(cache.largest_packages(packages, measured, target), ["bulky", "clang-sys", "serde_json"])

    def test_every_cleanup_command_selects_the_job_profile(self):
        packages = [{"name": "app", "id": "app", "source": None, "targets": [{"name": "app", "kind": ["lib"]}]},
                    {"name": "dep", "id": "dep", "source": "registry", "targets": [{"name": "dep", "kind": ["lib"]}]}]
        for profile, clean_profiles in (("debug", ["dev"]), ("release", ["release"]), ("debug-release", ["dev", "release"])):
            with self.subTest(profile=profile):
                for directory in ("debug", "release"):
                    self.file(self.paths[0] / f"{directory}/deps/libapp-hash.rlib", b"a" * 10_000)
                    self.file(self.paths[0] / f"{directory}/deps/libdep-hash.rlib", b"d" * 10_000)
                calls = []

                def execute(command, **_):
                    calls.append(command)
                    return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

                with patch.dict(cache.PROFILE_LIMITS, {profile: 1}):
                    result = cache.bound_payload(self.root, self.paths, profile, execute=execute)
                self.assertEqual(result["removed_dependency_packages"], 1)
                # Local cleanup and each eviction name the job's Cargo profile explicitly.
                self.assertEqual([command for command in calls if command[1] == "clean"],
                                 [["cargo", "clean", "--locked", "--profile", clean_profile, "--package", package]
                                  for package in ("app", "dep") for clean_profile in clean_profiles])

    def test_target_jobs_clean_both_layouts_and_refuse_unsafe_triples(self):
        packages = [{"name": "app", "id": "app", "source": None, "targets": [{"name": "app", "kind": ["lib"]}]},
                    {"name": "dep", "id": "dep", "source": "registry", "targets": [{"name": "dep", "kind": ["lib"]}]}]
        triple = "x86_64-unknown-linux-gnu"
        # A release job compiling with --target keeps artifacts under
        # target/<triple>/release; a debug job without --target is unchanged.
        for profile, target, layouts in (("release", triple, [[], ["--target", triple]]), ("debug", None, [[]])):
            with self.subTest(profile=profile):
                for directory in (f"{triple}/release", "debug"):
                    self.file(self.paths[0] / f"{directory}/deps/libapp-hash.rlib", b"a" * 10_000)
                    self.file(self.paths[0] / f"{directory}/deps/libdep-hash.rlib", b"d" * 10_000)
                calls = []

                def execute(command, **_):
                    calls.append(command)
                    return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

                with patch.dict(cache.PROFILE_LIMITS, {profile: 1}):
                    result = cache.bound_payload(self.root, self.paths, profile, execute=execute, target=target)
                self.assertEqual(result["removed_dependency_packages"], 1)
                self.assertEqual([command for command in calls if command[1] == "clean"],
                                 [["cargo", "clean", "--locked", "--profile", "dev" if profile == "debug" else "release",
                                   *layout, "--package", package] for package in ("app", "dep") for layout in layouts])
                self.assertNotIn(triple, json.dumps(result))
        # main() forwards only an empty or safe CACHE_TARGET; anything else is
        # refused before any Cargo command with the fixed save=false diagnostics.
        bound_payload, seen = cache.bound_payload, []

        def bound_without_cargo(*args, **kwargs):
            seen.append(kwargs.get("target"))
            return bound_payload(*args, execute=lambda *a, **k: b'{"packages": []}', **kwargs)

        for value, forwarded, save in ((triple, triple, "true"), ("", None, "true"), ("../evil triple", None, "false"),
                                       ("-", None, "false"), ("--x", None, "false")):
            with self.subTest(target=value):
                Path(self.env["GITHUB_OUTPUT"]).unlink(missing_ok=True)
                calls_before = len(seen)
                with patch.dict(os.environ, {"CACHE_PROFILE": "release", "CI_RUST_CACHE_PROFILE": "release", "CACHE_TARGET": value}), \
                        patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", bound_without_cargo), \
                        patch.object(subprocess, "run", side_effect=AssertionError("no subprocess may run")), \
                        patch.object(cache, "record") as record, patch("builtins.print"):
                    cache.main()
                result = record.call_args.kwargs["details"]
                self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), f"save={save}\n")
                if save == "true":
                    self.assertEqual(seen[-1], forwarded)
                else:
                    self.assertEqual(len(seen), calls_before)
                    self.assertEqual(result["reason"], "cache budget preparation unavailable")
                    self.assertEqual(result["failure_stage"], "unavailable")
                    self.assertNotIn("evil", json.dumps(result))

    def test_projection_mismatch_fails_in_its_own_stage(self):
        self.file(self.paths[1] / "src/registry/pure-1.0.0/lib.rs", b"pure")
        retained = self.file(self.paths[1] / "cache/dependency.crate", b"download")
        clear, bound_payload = cache.clear, cache.bound_payload

        def diverging_clear(paths):
            clear(paths)
            retained.unlink(missing_ok=True)  # physical pruning removes more than projected

        def bound_without_cargo(*args, **kwargs):
            # main() binds the real runner as the default executor; inject the
            # fake here so no Cargo process can ever run from this fixture.
            return bound_payload(*args, execute=lambda *a, **k: b'{"packages": []}', **kwargs)

        with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "CI_RUST_CACHE_PROFILE": "debug"}), \
                patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", bound_without_cargo), \
                patch.object(subprocess, "run", side_effect=AssertionError("no subprocess may run")), \
                patch.object(cache, "clear", diverging_clear), patch.object(cache, "record") as record, patch("builtins.print"):
            cache.main()
        result = record.call_args.kwargs["details"]
        self.assertFalse(result["save"])
        self.assertEqual(result["reason"], "cache budget preparation unavailable")
        self.assertEqual(result["failure_stage"], "projection_verification")
        self.assertEqual(result["stage_seconds"][-1]["stage"], "projection_verification")
        self.assertEqual(result["snapshots"][-1]["stage"], "after_source_pruning")
        self.assertEqual(result["limit_bytes"], cache.PROFILE_LIMITS["debug"])
        self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), "save=false\n")

    def budget_fixture(self):
        packages = [{"name": f"dep{i}", "id": f"dep{i}", "source": "registry", "targets": [{"name": f"dep{i}", "kind": ["lib"]}]}
                    for i in range(4)]
        libraries = {package["id"]: self.file(self.paths[0] / f"debug/deps/lib{package['name']}-hash.rlib", b"x" * 10_000)
                     for package in packages}
        self.file(self.paths[1] / "cache/index/dependency.crate", b"download")
        return packages, libraries

    def test_scheduling_stops_before_an_unaffordable_eviction_and_still_finalizes(self):
        packages, libraries = self.budget_fixture()
        clock = FakeClock()
        entries = cache.entries

        def slow_entries(paths, **kwargs):
            clock.now += 10  # each scan costs 10 s, checked against its deadline on completion
            return entries(paths, **kwargs)

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            clock.now += 25  # each cleanup costs 25 s of the 90 s scheduling budget
            libraries[command[-1]].unlink()
            return b""

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 20_000}), patch.object(cache, "entries", slow_entries):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute, clock=clock)
        # Two scans and one eviction end at 55 s. A second eviction would end
        # its scan exactly at the 90 s deadline and fail closed, so only the
        # one-measurement headroom refuses it; the loop stops with candidates
        # left and finalization still yields the downloads-only save.
        self.assertEqual(result["removed_dependency_packages"], 1)
        self.assertTrue(result["dropped_target"])
        self.assertTrue(result["save"])
        self.assertEqual(result["snapshots"][-1]["stage"], "after_target_fallback")

    def test_finalization_allowance_is_separate_and_fails_closed(self):
        clock = FakeClock()
        source = self.paths[1] / "src/registry/pure-1.0.0/lib.rs"
        clear = cache.clear

        def slow_clear(paths):
            clock.now += 25  # pruning and fallback removal count against finalization only
            clear(paths)

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            clock.now += 40
            libraries[command[-1]].unlink()
            return b""

        # Evictions use 80 s of the 90 s scheduling budget; the verification
        # scan then starts 105 s after the beginning, inside a 60 s allowance
        # but past a 20 s one. The 20,488-byte retained downloads fit 25,000.
        for finalize_seconds, saves in ((60, True), (20, False)):
            with self.subTest(finalize_seconds=finalize_seconds):
                packages, libraries = self.budget_fixture()
                self.file(source, b"pure")
                clock.now = 0.0
                with patch.dict(cache.PROFILE_LIMITS, {"debug": 25_000}), patch.object(cache, "clear", slow_clear), \
                        patch.object(cache, "FINALIZE_SECONDS", finalize_seconds):
                    if saves:
                        result = cache.bound_payload(self.root, self.paths, "debug", execute=execute, clock=clock)
                        self.assertTrue(result["save"])
                        self.assertTrue(result["dropped_target"])
                        self.assertEqual(result["removed_dependency_packages"], 2)
                        self.assertEqual(result["removed_source_directories"], 1)
                    else:
                        with self.assertRaises(TimeoutError):
                            cache.bound_payload(self.root, self.paths, "debug", execute=execute, clock=clock)
                        self.assertFalse(source.exists())

    def test_profiles_apply_their_own_limits(self):
        execute = lambda *a, **k: b'{"packages": []}'
        with patch.dict(cache.PROFILE_LIMITS, {"debug": 40_000, "release": 20_000}):
            # The same 37,288-byte compiled payload fits debug but not release.
            for profile, dropped in (("release", True), ("debug", False)):
                with self.subTest(profile=profile):
                    self.file(self.paths[0] / f"{profile}/deps/libdep-hash.rlib", b"d" * 25_000)
                    result = cache.bound_payload(self.root, self.paths, profile, execute=execute)
                    self.assertEqual(result["limit_bytes"], cache.PROFILE_LIMITS[profile])
                    self.assertEqual(result["dropped_target"], dropped)
                    self.assertTrue(result["save"])
        with self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "nightly", execute=execute)

    @unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                         "Set GITTURTLE_CACHE_CARGO_QA=1 for the real Cargo recovery fixture")
    def test_real_cargo_cleanup_rebuilds_local_crate_and_native_generated_output(self):
        self.file(self.root / "Cargo.toml", b'[workspace]\n[package]\nname="cache_recovery_fixture"\nversion="0.1.0"\nedition="2021"\n')
        self.file(self.root / "build.rs", b'use std::{env,fs,path::Path}; fn main() { fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("answer.rs"), "pub const ANSWER:u32=42;").unwrap(); }')
        self.file(self.root / "src/lib.rs", b'include!(concat!(env!("OUT_DIR"),"/answer.rs")); #[test] fn generated_answer(){assert_eq!(ANSWER,42);}')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True, capture_output=True)
        commands = [["cargo", "test", "--locked", "--offline", "--profile", profile] for profile in ("dev", "release")]
        for directory, command in zip(("debug", "release"), commands):
            first = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
            self.assertIn(b"1 passed", first.stdout)
            self.assertTrue(list(self.paths[0].glob(f"{directory}/build/*/out/answer.rs")))
        result = cache.bound_payload(self.root, self.paths, "debug-release")
        self.assertTrue(result["save"])
        for directory, command in zip(("debug", "release"), commands):
            self.assertFalse(list(self.paths[0].glob(f"{directory}/build/*/out/answer.rs")))
            second = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
            self.assertIn(b"1 passed", second.stdout)
            self.assertTrue(list(self.paths[0].glob(f"{directory}/build/*/out/answer.rs")))
            self.assertIn(b"Compiling cache_recovery_fixture", second.stderr)

    @unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                         "Set GITTURTLE_CACHE_CARGO_QA=1 for the real Cargo target-layout fixture")
    def test_real_cargo_release_target_layout_is_cleaned_only_with_the_triple(self):
        self.file(self.root / "Cargo.toml", b'[workspace]\n[package]\nname="cache_recovery_fixture"\nversion="0.1.0"\nedition="2021"\n')
        self.file(self.root / "build.rs", b'use std::{env,fs,path::Path}; fn main() { fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("answer.rs"), "pub const ANSWER:u32=42;").unwrap(); }')
        self.file(self.root / "src/lib.rs", b'include!(concat!(env!("OUT_DIR"),"/answer.rs")); #[test] fn generated_answer(){assert_eq!(ANSWER,42);}')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True, capture_output=True)
        version = subprocess.run(["rustc", "-vV"], cwd=self.root, check=True, capture_output=True).stdout.decode()
        host = next(line.split(": ", 1)[1].strip() for line in version.splitlines() if line.startswith("host: "))
        command = ["cargo", "test", "--locked", "--offline", "--profile", "release", "--target", host]
        first = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
        self.assertIn(b"1 passed", first.stdout)
        layout = self.paths[0] / host / "release"

        def artifacts():
            return sorted([*layout.glob("deps/*cache_recovery_fixture*"), *layout.glob(".fingerprint/cache_recovery_fixture-*"),
                           *layout.glob("build/cache_recovery_fixture-*/out/answer.rs")])

        built = artifacts()
        self.assertTrue(any(path.parent.name == "deps" for path in built))
        self.assertTrue(any(path.parent.name == ".fingerprint" for path in built))
        self.assertTrue(any(path.name == "answer.rs" for path in built))
        # Without the triple, pinned Cargo cleans only the host layout: every
        # triple-layout artifact survives, which is the hosted release no-op.
        untouched = cache.bound_payload(self.root, self.paths, "release")
        self.assertTrue(untouched["save"])
        self.assertEqual(artifacts(), built)
        cleaned = cache.bound_payload(self.root, self.paths, "release", target=host)
        self.assertTrue(cleaned["save"])
        self.assertEqual(artifacts(), [])
        self.assertEqual([row["stage"] for row in cleaned["stage_seconds"] if row["stage"].startswith("local_cleanup")],
                         ["local_cleanup_release", "local_cleanup_release_target"])
        self.assertNotIn(host, json.dumps(cleaned))
        second = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
        self.assertIn(b"1 passed", second.stdout)
        self.assertIn(b"Compiling cache_recovery_fixture", second.stderr)
        self.assertTrue(list(layout.glob("build/cache_recovery_fixture-*/out/answer.rs")))


if __name__ == "__main__":
    unittest.main()
